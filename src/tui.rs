use std::{
    cell::RefCell,
    collections::HashMap,
    collections::hash_map::DefaultHasher,
    env,
    hash::{Hash, Hasher},
    io::{self, Stdout, Write},
    time::Duration,
};

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::Alignment,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols::border,
    text::{Line, Span, Text},
    widgets::{Block, Borders, Cell, Clear, Padding, Paragraph, Row, Table},
};

use crate::app::{
    App, BoardRowTone, CommandCenterPrimaryTrigger, ThemeColor, ThemeOverrides, ThemePreset,
    UiSettings,
};

type TuiTerminal = Terminal<CrosstermBackend<Stdout>>;
const INPUT_POLL_TIMEOUT: Duration = Duration::from_millis(33);
const QUEUED_INPUT_DRAIN_LIMIT: usize = 64;
const BOARD_LOCATION_MIN_WIDTH: u16 = 12;
const BOARD_LOCATION_MAX_WIDTH: u16 = 16;

thread_local! {
    static FITTED_PANEL_LINES_CACHE: RefCell<Option<FittedPanelLinesCache>> = const { RefCell::new(None) };
}

#[derive(Clone, Debug)]
struct FittedPanelLinesCache {
    width: u16,
    source_hash: u64,
    source_len: usize,
    fitted: Vec<String>,
}

const ASCII_BORDER_SET: border::Set<'static> = border::Set {
    top_left: "+",
    top_right: "+",
    bottom_left: "+",
    bottom_right: "+",
    vertical_left: "|",
    vertical_right: "|",
    horizontal_top: "-",
    horizontal_bottom: "-",
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TerminalProfile {
    ascii_borders: bool,
    color: bool,
}

impl TerminalProfile {
    fn modern() -> Self {
        Self {
            ascii_borders: false,
            color: true,
        }
    }

    fn from_env() -> Self {
        let env = std::env::vars().collect::<HashMap<_, _>>();
        Self::from_env_map(&env)
    }

    fn from_env_map(env: &HashMap<String, String>) -> Self {
        let term = env.get("TERM").map(String::as_str).unwrap_or_default();
        let dumb = term.eq_ignore_ascii_case("dumb");
        let ascii_borders = dumb || !locale_is_utf8(env);
        let color = !dumb
            && !env.contains_key("NO_COLOR")
            && env.get("CLICOLOR").is_none_or(|value| value.trim() != "0");

        Self {
            ascii_borders,
            color,
        }
    }
}

impl Default for TerminalProfile {
    fn default() -> Self {
        Self::modern()
    }
}

fn locale_is_utf8(env: &HashMap<String, String>) -> bool {
    let locale = ["LC_ALL", "LC_CTYPE", "LANG"]
        .into_iter()
        .filter_map(|key| env.get(key))
        .find(|value| !value.trim().is_empty());

    match locale {
        Some(value) => {
            let value = value.to_ascii_uppercase();
            value.contains("UTF-8") || value.contains("UTF8")
        }
        None => true,
    }
}

#[derive(Clone, Copy)]
struct Theme {
    mono: bool,
    color: bool,
    ascii_borders: bool,
    text: Color,
    muted: Color,
    accent: Color,
    success: Color,
    warning: Color,
    danger: Color,
    border: Color,
    surface: Color,
    selected_fg: Color,
    selected_bg: Color,
}

impl Theme {
    #[cfg(test)]
    fn from_preset(preset: ThemePreset) -> Self {
        Self::from_preset_with_profile(preset, TerminalProfile::modern())
    }

    #[cfg(test)]
    fn from_settings(settings: &UiSettings) -> Self {
        Self::from_settings_with_profile(settings, TerminalProfile::modern())
    }

    fn from_settings_with_profile(settings: &UiSettings, profile: TerminalProfile) -> Self {
        let mut theme = Self::from_preset_with_profile(settings.active_theme_preset(), profile);
        theme.apply_overrides(&settings.theme.overrides);
        theme
    }

    fn from_preset_with_profile(preset: ThemePreset, profile: TerminalProfile) -> Self {
        match preset {
            ThemePreset::Calm => Self {
                mono: false,
                color: profile.color,
                ascii_borders: profile.ascii_borders,
                text: Color::Reset,
                muted: Color::Gray,
                accent: Color::LightBlue,
                success: Color::LightCyan,
                warning: Color::Yellow,
                danger: Color::LightRed,
                border: Color::Gray,
                surface: Color::DarkGray,
                selected_fg: Color::White,
                selected_bg: Color::DarkGray,
            },
            ThemePreset::Contrast => Self {
                mono: false,
                color: profile.color,
                ascii_borders: profile.ascii_borders,
                text: Color::White,
                muted: Color::Gray,
                accent: Color::LightCyan,
                success: Color::LightGreen,
                warning: Color::LightYellow,
                danger: Color::LightRed,
                border: Color::Gray,
                surface: Color::DarkGray,
                selected_fg: Color::White,
                selected_bg: Color::DarkGray,
            },
            ThemePreset::Mono => Self {
                mono: true,
                color: profile.color,
                ascii_borders: profile.ascii_borders,
                text: Color::White,
                muted: Color::Gray,
                accent: Color::White,
                success: Color::White,
                warning: Color::White,
                danger: Color::White,
                border: Color::Gray,
                surface: Color::Gray,
                selected_fg: Color::White,
                selected_bg: Color::Black,
            },
            ThemePreset::TerminalNative => Self {
                mono: false,
                color: profile.color,
                ascii_borders: profile.ascii_borders,
                text: Color::Reset,
                muted: Color::DarkGray,
                accent: Color::Blue,
                success: Color::Green,
                warning: Color::Yellow,
                danger: Color::Red,
                border: Color::DarkGray,
                surface: Color::DarkGray,
                selected_fg: Color::Reset,
                selected_bg: Color::Reset,
            },
            ThemePreset::CatppuccinLatte => Self::rgb_palette(
                profile, 0x4C4F69, 0x6C6F85, 0x1E66F5, 0x40A02B, 0xDF8E1D, 0xD20F39, 0xACB0BE,
                0xCCD0DA, 0x4C4F69, 0xCCD0DA,
            ),
            ThemePreset::CatppuccinMocha => Self::rgb_palette(
                profile, 0xCDD6F4, 0x7F849C, 0x89B4FA, 0xA6E3A1, 0xF9E2AF, 0xF38BA8, 0x585B70,
                0x313244, 0xCDD6F4, 0x45475A,
            ),
            ThemePreset::TokyoNight => Self::rgb_palette(
                profile, 0xC0CAF5, 0x565F89, 0x7AA2F7, 0x9ECE6A, 0xE0AF68, 0xF7768E, 0x3B4261,
                0x292E42, 0xC0CAF5, 0x3B4261,
            ),
            ThemePreset::GruvboxDark => Self::rgb_palette(
                profile, 0xEBDBB2, 0x928374, 0x83A598, 0xB8BB26, 0xFABD2F, 0xFB4934, 0x665C54,
                0x3C3836, 0xEBDBB2, 0x504945,
            ),
            ThemePreset::GruvboxLight => Self::rgb_palette(
                profile, 0x3C3836, 0x928374, 0x076678, 0x79740E, 0xB57614, 0x9D0006, 0xD5C4A1,
                0xEBDBB2, 0x3C3836, 0xD5C4A1,
            ),
            ThemePreset::Nord => Self::rgb_palette(
                profile, 0xD8DEE9, 0x4C566A, 0x88C0D0, 0xA3BE8C, 0xEBCB8B, 0xBF616A, 0x4C566A,
                0x3B4252, 0xECEFF4, 0x434C5E,
            ),
            ThemePreset::RosePine => Self::rgb_palette(
                profile, 0xE0DEF4, 0x6E6A86, 0x9CCFD8, 0x31748F, 0xF6C177, 0xEB6F92, 0x403D52,
                0x26233A, 0xE0DEF4, 0x403D52,
            ),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn rgb_palette(
        profile: TerminalProfile,
        text: u32,
        muted: u32,
        accent: u32,
        success: u32,
        warning: u32,
        danger: u32,
        border: u32,
        surface: u32,
        selected_fg: u32,
        selected_bg: u32,
    ) -> Self {
        Self {
            mono: false,
            color: profile.color,
            ascii_borders: profile.ascii_borders,
            text: Color::from_u32(text),
            muted: Color::from_u32(muted),
            accent: Color::from_u32(accent),
            success: Color::from_u32(success),
            warning: Color::from_u32(warning),
            danger: Color::from_u32(danger),
            border: Color::from_u32(border),
            surface: Color::from_u32(surface),
            selected_fg: Color::from_u32(selected_fg),
            selected_bg: Color::from_u32(selected_bg),
        }
    }

    fn apply_overrides(&mut self, overrides: &ThemeOverrides) {
        if let Some(color) = overrides.text {
            self.text = theme_color(color);
        }
        if let Some(color) = overrides.muted {
            self.muted = theme_color(color);
        }
        if let Some(color) = overrides.accent {
            self.accent = theme_color(color);
        }
        if let Some(color) = overrides.success {
            self.success = theme_color(color);
        }
        if let Some(color) = overrides.warning {
            self.warning = theme_color(color);
        }
        if let Some(color) = overrides.danger {
            self.danger = theme_color(color);
        }
        if let Some(color) = overrides.surface {
            self.surface = theme_color(color);
        }
        if let Some(color) = overrides.border {
            self.border = theme_color(color);
        }
        if let Some(color) = overrides.selected_fg {
            self.selected_fg = theme_color(color);
        }
        if let Some(color) = overrides.selected_bg {
            self.selected_bg = theme_color(color);
        }
    }

    fn block(self, title: &str) -> Block<'static> {
        self.with_border_set(
            Block::default()
                .borders(Borders::ALL)
                .border_style(self.fg(self.border))
                .title(title.to_owned())
                .title_style(self.fg(self.text).add_modifier(Modifier::BOLD)),
        )
    }

    fn focused_block(self, title: &str) -> Block<'static> {
        self.with_border_set(
            Block::default()
                .borders(Borders::ALL)
                .border_style(self.fg(self.accent))
                .title(title.to_owned())
                .title_style(self.fg(self.accent).add_modifier(Modifier::BOLD)),
        )
    }

    fn with_border_set(self, block: Block<'static>) -> Block<'static> {
        if self.ascii_borders {
            block.border_set(ASCII_BORDER_SET)
        } else {
            block
        }
    }

    fn fg(self, color: Color) -> Style {
        if self.color {
            Style::default().fg(color)
        } else {
            Style::default()
        }
    }

    fn bg(self, style: Style, color: Color) -> Style {
        if self.color { style.bg(color) } else { style }
    }

    fn brand_style(self) -> Style {
        self.fg(self.accent).add_modifier(Modifier::BOLD)
    }

    fn body_style(self) -> Style {
        self.fg(self.text)
    }

    fn accent_style(self) -> Style {
        self.fg(self.accent)
    }

    fn muted_style(self) -> Style {
        self.fg(self.muted)
    }

    fn surface_style(self) -> Style {
        self.fg(self.surface)
    }

    fn success_style(self) -> Style {
        self.fg(self.success)
    }

    fn warning_style(self) -> Style {
        self.fg(self.warning)
    }

    fn danger_style(self) -> Style {
        self.fg(self.danger)
    }

    fn section_style(self) -> Style {
        self.fg(self.accent).add_modifier(Modifier::BOLD)
    }

    fn table_header_style(self) -> Style {
        self.fg(self.text).add_modifier(Modifier::BOLD)
    }

    fn default_row_style(self) -> Style {
        self.fg(self.text)
    }

    fn subdued_row_style(self) -> Style {
        self.fg(self.muted).add_modifier(Modifier::DIM)
    }

    fn selected_row_style(self) -> Style {
        if self.selected_fg == Color::Reset && self.selected_bg == Color::Reset {
            return Style::default().add_modifier(Modifier::BOLD | Modifier::REVERSED);
        }
        let style = self
            .bg(self.fg(self.selected_fg), self.selected_bg)
            .add_modifier(Modifier::BOLD);
        if self.mono || !self.color {
            style.add_modifier(Modifier::REVERSED)
        } else {
            style
        }
    }

    fn confirm_row_style(self) -> Style {
        self.fg(self.warning).add_modifier(Modifier::BOLD)
    }

    fn targeted_row_style(self) -> Style {
        let style = self.fg(self.success).add_modifier(Modifier::BOLD);
        if self.mono || !self.color {
            style.add_modifier(Modifier::UNDERLINED)
        } else {
            style
        }
    }

    fn attention_row_style(self) -> Style {
        self.fg(self.warning).add_modifier(Modifier::BOLD)
    }

    fn watching_row_style(self) -> Style {
        let style = self.fg(self.muted).add_modifier(Modifier::BOLD);
        if self.mono || !self.color {
            style.add_modifier(Modifier::DIM)
        } else {
            style
        }
    }

    fn alert_row_style(self) -> Style {
        let style = self.fg(self.danger).add_modifier(Modifier::BOLD);
        if self.mono || !self.color {
            style.add_modifier(Modifier::REVERSED)
        } else {
            style
        }
    }
}

fn theme_color(color: ThemeColor) -> Color {
    match color {
        ThemeColor::Reset => Color::Reset,
        ThemeColor::Black => Color::Black,
        ThemeColor::Red => Color::Red,
        ThemeColor::Green => Color::Green,
        ThemeColor::Yellow => Color::Yellow,
        ThemeColor::Blue => Color::Blue,
        ThemeColor::Magenta => Color::Magenta,
        ThemeColor::Cyan => Color::Cyan,
        ThemeColor::Gray => Color::Gray,
        ThemeColor::DarkGray => Color::DarkGray,
        ThemeColor::LightRed => Color::LightRed,
        ThemeColor::LightGreen => Color::LightGreen,
        ThemeColor::LightYellow => Color::LightYellow,
        ThemeColor::LightBlue => Color::LightBlue,
        ThemeColor::LightMagenta => Color::LightMagenta,
        ThemeColor::LightCyan => Color::LightCyan,
        ThemeColor::White => Color::White,
        ThemeColor::Indexed(index) => Color::Indexed(index),
        ThemeColor::Rgb(red, green, blue) => Color::Rgb(red, green, blue),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BoardLayoutMode {
    Full,
    Standard,
    Compact,
}

impl BoardLayoutMode {
    fn for_width(width: u16) -> Self {
        if width >= 120 {
            Self::Full
        } else if width >= 68 {
            Self::Standard
        } else {
            Self::Compact
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BodyLayoutMode {
    SplitColumns,
    Stack,
}

impl BodyLayoutMode {
    fn for_area(
        width: u16,
        height: u16,
        context_priority: bool,
        content_pressure: bool,
        dense_fleet: bool,
    ) -> Self {
        if width < 84
            || (width < 100 && height >= 14)
            || (width < 120 && height >= 18)
            || (width < 132
                && height >= 22
                && content_pressure
                && (context_priority || !dense_fleet))
        {
            Self::Stack
        } else {
            Self::SplitColumns
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ResolvedBodyLayout {
    mode: BodyLayoutMode,
    board_percent: u16,
    context_percent: u16,
}

impl ResolvedBodyLayout {
    fn for_app(app: &App, body: Rect, profile: LayoutProfile) -> Self {
        let context_priority = app.is_output_view_active()
            || app.is_browse_view_active()
            || app.is_command_center_active();
        let usable_rows = usize::from(body.height.saturating_sub(4).max(1));
        let context_pressure_rows = usable_rows.saturating_sub(8).max(8);
        let dense_fleet = app.layout_visible_pane_count() > usable_rows;
        let content_pressure = app.layout_context_line_count() >= context_pressure_rows
            || app.is_output_view_active()
            || app.is_browse_view_active()
            || app.is_command_center_active();
        let mode = match app.layout_preset() {
            crate::app::LayoutPreset::Auto => BodyLayoutMode::for_area(
                body.width,
                body.height,
                context_priority,
                content_pressure,
                dense_fleet,
            ),
            crate::app::LayoutPreset::Horizontal => BodyLayoutMode::SplitColumns,
            crate::app::LayoutPreset::Vertical => BodyLayoutMode::Stack,
        };
        let (board_percent, context_percent) = match mode {
            BodyLayoutMode::SplitColumns => split_body_percentages(
                body.width,
                context_priority,
                content_pressure,
                dense_fleet,
                profile,
            ),
            BodyLayoutMode::Stack => stack_body_percentages(
                body.width,
                body.height,
                context_priority,
                content_pressure,
                dense_fleet,
                profile,
            ),
        };

        Self {
            mode,
            board_percent,
            context_percent,
        }
    }
}

fn split_body_percentages(
    width: u16,
    context_priority: bool,
    content_pressure: bool,
    dense_fleet: bool,
    profile: LayoutProfile,
) -> (u16, u16) {
    if context_priority && content_pressure && width < 144 {
        (42, 58)
    } else if context_priority && content_pressure {
        (44, 56)
    } else if context_priority {
        (46, 54)
    } else if content_pressure && !dense_fleet && width < 144 {
        (48, 52)
    } else if content_pressure {
        (profile.board_percent, profile.context_percent)
    } else if dense_fleet && width >= 150 {
        (58, 42)
    } else if dense_fleet {
        (56, 44)
    } else {
        (profile.board_percent, profile.context_percent)
    }
}

fn stack_body_percentages(
    width: u16,
    height: u16,
    context_priority: bool,
    content_pressure: bool,
    dense_fleet: bool,
    profile: LayoutProfile,
) -> (u16, u16) {
    if context_priority && content_pressure {
        (36, 64)
    } else if context_priority || content_pressure {
        (42, 58)
    } else if height >= 30 && width <= 84 {
        (36, 64)
    } else if dense_fleet || height <= 14 {
        (54, 46)
    } else {
        (
            profile.stacked_board_percent,
            profile.stacked_context_percent,
        )
    }
}

pub async fn run(app: &mut App) -> Result<()> {
    let mut terminal = init_terminal()?;
    let result = run_loop(&mut terminal, app).await;
    restore_terminal(&mut terminal)?;
    result
}

async fn run_loop(terminal: &mut TuiTerminal, app: &mut App) -> Result<()> {
    let terminal_profile = TerminalProfile::from_env();
    let peek_toggle_keys = PeekToggleKeys::from_env();
    let mut peek_prefix_pending = false;
    while !app.should_quit() {
        terminal.draw(|frame| draw_with_profile(frame, app, terminal_profile))?;
        if app.take_pending_bell() {
            ring_terminal_bell()?;
        }

        if event::poll(INPUT_POLL_TIMEOUT)? {
            drain_queued_input(app, peek_toggle_keys.as_ref(), &mut peek_prefix_pending).await?;
        } else {
            app.tick().await?;
        }
    }

    Ok(())
}

async fn drain_queued_input(
    app: &mut App,
    peek_toggle_keys: Option<&PeekToggleKeys>,
    peek_prefix_pending: &mut bool,
) -> Result<()> {
    for _ in 0..QUEUED_INPUT_DRAIN_LIMIT {
        match event::read()? {
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                handle_key_event(app, key, peek_toggle_keys, peek_prefix_pending).await?;
            }
            _ => {}
        }

        if app.should_quit() || !event::poll(Duration::from_millis(0))? {
            break;
        }
    }

    Ok(())
}

async fn handle_key_event(
    app: &mut App,
    key: KeyEvent,
    peek_toggle_keys: Option<&PeekToggleKeys>,
    peek_prefix_pending: &mut bool,
) -> Result<()> {
    if let Some(toggle_keys) = peek_toggle_keys
        && handle_peek_toggle_key(app, key, toggle_keys, peek_prefix_pending)
    {
        return Ok(());
    }

    handle_key_press(app, normalized_app_key_code(key)).await
}

fn normalized_app_key_code(key: KeyEvent) -> KeyCode {
    match key.code {
        KeyCode::Char('\r' | '\n') => KeyCode::Enter,
        KeyCode::Char(ch)
            if key.modifiers.contains(KeyModifiers::CONTROL)
                && matches!(ch.to_ascii_lowercase(), 'j' | 'm') =>
        {
            KeyCode::Enter
        }
        code => code,
    }
}

fn handle_peek_toggle_key(
    app: &mut App,
    key: KeyEvent,
    toggle_keys: &PeekToggleKeys,
    prefix_pending: &mut bool,
) -> bool {
    if key.code == KeyCode::Esc {
        *prefix_pending = false;
        app.request_quit();
        return true;
    }

    if *prefix_pending {
        *prefix_pending = false;
        if toggle_keys.key.matches(key) {
            app.request_quit();
            return true;
        }
    }

    if toggle_keys
        .prefixes
        .iter()
        .any(|prefix| prefix.matches(key))
    {
        *prefix_pending = true;
        return true;
    }

    false
}

async fn handle_key_press(app: &mut App, code: KeyCode) -> Result<()> {
    if let Err(error) = handle_key_press_inner(app, code).await {
        app.report_action_error(&error);
    }

    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PeekToggleKeys {
    prefixes: Vec<KeyPattern>,
    key: KeyPattern,
}

impl PeekToggleKeys {
    fn from_env() -> Option<Self> {
        if !env_flag_enabled(env::var("MUXBOARD_TMUX_PEEK_TOGGLE").ok().as_deref()) {
            return None;
        }

        let prefix = env::var("MUXBOARD_TMUX_PEEK_PREFIX").ok()?;
        let key = env::var("MUXBOARD_TMUX_PEEK_KEY").ok()?;
        let mut prefixes = Vec::new();
        if let Some(prefix) = KeyPattern::parse_tmux_token(&prefix) {
            prefixes.push(prefix);
        }
        if let Ok(prefix2) = env::var("MUXBOARD_TMUX_PEEK_PREFIX2")
            && let Some(prefix2) = KeyPattern::parse_tmux_token(&prefix2)
        {
            prefixes.push(prefix2);
        }
        if prefixes.is_empty() {
            return None;
        }
        Some(Self {
            prefixes,
            key: KeyPattern::parse_tmux_token(&key)?,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum KeyPattern {
    Char(char),
    Ctrl(char),
}

impl KeyPattern {
    fn parse_tmux_token(token: &str) -> Option<Self> {
        let token = token.trim();
        if token.eq_ignore_ascii_case("none") {
            return None;
        }

        if let Some(rest) = token
            .strip_prefix("C-")
            .or_else(|| token.strip_prefix("c-"))
        {
            if rest.eq_ignore_ascii_case("space") {
                return Some(Self::Ctrl(' '));
            }
            if rest.chars().count() == 1 {
                return rest
                    .chars()
                    .next()
                    .map(|ch| Self::Ctrl(ch.to_ascii_lowercase()));
            }
        }

        if token.eq_ignore_ascii_case("space") {
            return Some(Self::Char(' '));
        }

        if token.chars().count() == 1 {
            return token.chars().next().map(Self::Char);
        }

        None
    }

    fn matches(self, key: KeyEvent) -> bool {
        match self {
            Self::Char(expected) => matches!(key.code, KeyCode::Char(actual) if {
                actual == expected
                    || (expected.is_ascii_uppercase()
                        && key.modifiers.contains(KeyModifiers::SHIFT)
                        && actual.to_ascii_uppercase() == expected)
            }),
            Self::Ctrl(expected) => {
                matches!(key.code, KeyCode::Char(actual) if {
                    let modified = key.modifiers.contains(KeyModifiers::CONTROL)
                        && actual.eq_ignore_ascii_case(&expected);
                    let literal_control = expected.is_ascii()
                        && actual == char::from((expected.to_ascii_lowercase() as u8) & 0x1f);
                    modified || literal_control
                })
            }
        }
    }
}

fn env_flag_enabled(value: Option<&str>) -> bool {
    value.map(str::trim).is_some_and(|value| {
        matches!(
            value.to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

async fn handle_key_press_inner(app: &mut App, code: KeyCode) -> Result<()> {
    if app.is_theme_picker_active() {
        match code {
            KeyCode::Esc => {
                app.cancel_theme_picker();
            }
            KeyCode::Enter => {
                app.submit_theme_picker();
            }
            _ if matches_any(app.keybindings(), &code, |bindings| &bindings.quit) => {
                app.quit_from_theme_picker();
            }
            _ if matches_any(app.keybindings(), &code, |bindings| &bindings.move_down) => {
                app.select_next_theme_option();
            }
            _ if matches_any(app.keybindings(), &code, |bindings| &bindings.move_up) => {
                app.select_previous_theme_option();
            }
            _ => {}
        }
        return Ok(());
    }

    if app.is_help_overlay_active() {
        match code {
            KeyCode::Esc | KeyCode::Char('?') => {
                app.close_help_overlay();
            }
            _ if matches_any(app.keybindings(), &code, |bindings| &bindings.action_layout) => {
                app.close_help_overlay();
                app.cycle_layout_preset();
            }
            _ if matches_any(app.keybindings(), &code, |bindings| &bindings.quit) => {
                app.request_quit();
            }
            _ => {
                app.close_help_overlay();
                return Box::pin(handle_key_press_inner(app, code)).await;
            }
        }
        return Ok(());
    }

    if app.has_pending_dispatch() {
        match code {
            KeyCode::Esc => {
                app.cancel_pending_dispatch();
            }
            KeyCode::Enter => app.confirm_pending_dispatch().await?,
            KeyCode::Char('?') => app.toggle_help_overlay(),
            _ => {}
        }
        return Ok(());
    }

    if app.is_macro_assign_active() {
        match code {
            KeyCode::Esc => {
                app.cancel_macro_assign();
            }
            KeyCode::Char('?') => {
                app.toggle_help_overlay();
            }
            _ => {
                if let Some(token) = key_token(&code)
                    && let Some(slot) = app.macro_slot_for_key_token(&token)
                {
                    app.assign_recent_command_to_slot(slot);
                }
            }
        }
        return Ok(());
    }

    if app.is_action_menu_active() {
        let mut dismiss_after_action = false;
        match code {
            KeyCode::Esc => {
                app.close_action_menu();
            }
            KeyCode::Backspace if app.has_view_narrowing() => {
                app.clear_view_scope();
                dismiss_after_action = true;
            }
            KeyCode::Char('?') => {
                app.toggle_help_overlay();
            }
            _ if matches_any(app.keybindings(), &code, |bindings| &bindings.actions) => {
                app.close_action_menu();
            }
            _ if matches_any(app.keybindings(), &code, |bindings| &bindings.focus)
                && app.action_menu_has_visible_selection() =>
            {
                if app.is_output_view_active() {
                    app.go_back();
                } else {
                    app.focus_selected_pane().await?;
                }
                dismiss_after_action = true;
            }
            _ if app.action_menu_has_actionable_targets()
                && matches_any(app.keybindings(), &code, |bindings| &bindings.command) =>
            {
                if app.selected_pane_can_reply_text() {
                    app.begin_command_input();
                } else {
                    app.show_send_view();
                }
                dismiss_after_action = true;
            }
            _ if matches_any(app.keybindings(), &code, |bindings| {
                &bindings.action_view_browse
            }) =>
            {
                app.show_browse_view();
                dismiss_after_action = true;
            }
            _ if matches_any(app.keybindings(), &code, |bindings| {
                &bindings.action_view_command_center
            }) =>
            {
                app.show_command_center();
                dismiss_after_action = true;
            }
            _ if app.action_menu_has_actionable_targets()
                && matches_any(app.keybindings(), &code, |bindings| &bindings.summaries) =>
            {
                app.request_target_summaries().await?;
                dismiss_after_action = true;
            }
            _ if matches_any(app.keybindings(), &code, |bindings| &bindings.refresh) => {
                app.refresh().await?;
                dismiss_after_action = true;
            }
            _ if app.action_menu_has_sortable_panes()
                && matches_any(app.keybindings(), &code, |bindings| &bindings.action_sort) =>
            {
                app.cycle_sort_mode();
                dismiss_after_action = true;
            }
            _ if app.action_menu_has_sortable_panes()
                && matches_any(app.keybindings(), &code, |bindings| &bindings.action_filter) =>
            {
                app.cycle_filter_mode();
                dismiss_after_action = true;
            }
            _ if app.action_menu_can_clear_marks()
                && matches_any(app.keybindings(), &code, |bindings| &bindings.clear_marks) =>
            {
                app.clear_marked_panes();
                dismiss_after_action = true;
            }
            _ if app.action_menu_can_save_group()
                && matches_any(app.keybindings(), &code, |bindings| {
                    &bindings.action_group_save
                }) =>
            {
                app.begin_group_save_input()
            }
            _ if app.action_menu_can_load_group()
                && matches_any(app.keybindings(), &code, |bindings| {
                    &bindings.action_group_load
                }) =>
            {
                app.open_fleet_picker();
            }
            _ if app.action_menu_can_delete_group()
                && matches_any(app.keybindings(), &code, |bindings| {
                    &bindings.action_group_delete
                }) =>
            {
                app.delete_selected_target_group();
                dismiss_after_action = true;
            }
            _ if app.action_menu_has_visible_selection()
                && matches_any(app.keybindings(), &code, |bindings| &bindings.jump) =>
            {
                app.jump_to_selected_pane().await?;
                dismiss_after_action = true;
            }
            _ if app.action_menu_has_visible_selection()
                && matches_any(app.keybindings(), &code, |bindings| {
                    &bindings.action_launch_agent
                }) =>
            {
                app.begin_launch_input();
            }
            _ if app.action_menu_can_target_lane()
                && matches_any(app.keybindings(), &code, |bindings| {
                    &bindings.action_lane_target
                }) =>
            {
                app.toggle_fanout_mode();
                dismiss_after_action = true;
            }
            _ if matches_any(app.keybindings(), &code, |bindings| {
                &bindings.action_metrics
            }) =>
            {
                app.toggle_metrics_mode();
                dismiss_after_action = true;
            }
            _ if matches_any(app.keybindings(), &code, |bindings| &bindings.action_layout) => {
                app.cycle_layout_preset();
                dismiss_after_action = true;
            }
            _ if matches_any(app.keybindings(), &code, |bindings| {
                &bindings.action_desktop_notifications
            }) =>
            {
                app.toggle_desktop_notifications();
                dismiss_after_action = true;
            }
            _ if matches_any(app.keybindings(), &code, |bindings| &bindings.action_bell) => {
                app.toggle_bell_notifications();
                dismiss_after_action = true;
            }
            _ if matches_any(app.keybindings(), &code, |bindings| {
                &bindings.action_alert_debounce
            }) =>
            {
                app.cycle_alert_debounce();
                dismiss_after_action = true;
            }
            _ if matches_any(app.keybindings(), &code, |bindings| {
                &bindings.action_alert_policy
            }) =>
            {
                app.cycle_alert_policy();
                dismiss_after_action = true;
            }
            _ if app.action_menu_can_ack_selected()
                && matches_any(app.keybindings(), &code, |bindings| {
                    &bindings.action_ack_selected
                }) =>
            {
                app.acknowledge_selected_attention();
                dismiss_after_action = true;
            }
            _ if app.action_menu_can_clear_selected_ack()
                && matches_any(app.keybindings(), &code, |bindings| {
                    &bindings.action_ack_clear_selected
                }) =>
            {
                app.clear_selected_acknowledgement();
                dismiss_after_action = true;
            }
            _ if app.action_menu_can_ack_all()
                && matches_any(app.keybindings(), &code, |bindings| {
                    &bindings.action_ack_all
                }) =>
            {
                app.acknowledge_all_attention();
                dismiss_after_action = true;
            }
            _ if app.action_menu_can_clear_all_acks()
                && matches_any(app.keybindings(), &code, |bindings| {
                    &bindings.action_ack_clear_all
                }) =>
            {
                app.clear_all_acknowledgements();
                dismiss_after_action = true;
            }
            _ if app.action_menu_can_continue_waiting()
                && matches_any(app.keybindings(), &code, |bindings| {
                    &bindings.action_enter_queue
                }) =>
            {
                app.send_enter_to_attention_queue().await?;
                dismiss_after_action = true;
            }
            _ if matches_any(app.keybindings(), &code, |bindings| &bindings.action_zoom)
                && app.action_menu_has_visible_selection() =>
            {
                app.toggle_selected_zoom().await?;
                dismiss_after_action = true;
            }
            _ if app.action_menu_has_visible_selection()
                && matches_any(app.keybindings(), &code, |bindings| {
                    &bindings.action_send_enter
                }) =>
            {
                app.send_enter_to_selected().await?;
                dismiss_after_action = true;
            }
            _ if app.action_menu_can_answer_choice()
                && matches_any(app.keybindings(), &code, |bindings| {
                    &bindings.action_send_yes
                }) =>
            {
                app.send_yes_to_selected().await?;
                dismiss_after_action = true;
            }
            _ if app.action_menu_can_answer_choice()
                && matches_any(app.keybindings(), &code, |bindings| {
                    &bindings.action_send_no
                }) =>
            {
                app.send_no_to_selected().await?;
                dismiss_after_action = true;
            }
            _ => {}
        }
        if dismiss_after_action {
            app.dismiss_action_menu();
        }
        return Ok(());
    }

    if app.is_fleet_picker_active() {
        match code {
            KeyCode::Esc => {
                app.close_fleet_picker();
            }
            KeyCode::Char('?') => {
                app.toggle_help_overlay();
            }
            KeyCode::Enter => app.submit_fleet_picker(),
            _ if matches_any(app.keybindings(), &code, |bindings| {
                &bindings.action_group_load
            }) =>
            {
                app.submit_fleet_picker();
            }
            _ if matches_any(app.keybindings(), &code, |bindings| {
                &bindings.action_group_delete
            }) =>
            {
                app.delete_fleet_picker_selection();
            }
            _ if matches_any(app.keybindings(), &code, |bindings| &bindings.move_down) => {
                app.select_next_fleet();
            }
            _ if matches_any(app.keybindings(), &code, |bindings| &bindings.move_up) => {
                app.select_previous_fleet();
            }
            _ => {}
        }
        return Ok(());
    }

    if app.is_launch_input_active() {
        match code {
            KeyCode::Esc => {
                app.cancel_launch_input();
            }
            KeyCode::Enter => app.submit_launch_input().await?,
            KeyCode::Tab => app.cycle_launch_preset(true),
            KeyCode::BackTab => app.cycle_launch_preset(false),
            KeyCode::Backspace => app.pop_launch_char(),
            KeyCode::Char(ch) => app.push_launch_char(ch),
            _ => {}
        }
        return Ok(());
    }

    if app.is_group_input_active() {
        match code {
            KeyCode::Esc => {
                app.cancel_group_input();
            }
            KeyCode::Enter => app.submit_group_input(),
            KeyCode::Backspace => app.pop_group_name_char(),
            KeyCode::Char(ch) => app.push_group_name_char(ch),
            _ => {}
        }
        return Ok(());
    }

    if app.is_command_input_active() {
        match code {
            KeyCode::Esc => {
                app.cancel_command_input();
            }
            KeyCode::Enter => app.submit_command_input().await?,
            KeyCode::Backspace => app.pop_command_char(),
            _ if app.command_input_can_repeat_recent()
                && matches_any(app.keybindings(), &code, |bindings| &bindings.repeat_last) =>
            {
                app.cancel_command_input();
                app.repeat_last_command().await?;
            }
            KeyCode::Char(ch) => app.push_command_char(ch),
            _ => {}
        }
        return Ok(());
    }

    if app.is_search_input_active() {
        match code {
            KeyCode::Esc => {
                app.cancel_search();
            }
            KeyCode::Enter => app.finish_search(),
            KeyCode::Backspace => app.pop_search_char(),
            KeyCode::Char(ch) => app.push_search_char(ch),
            _ => {}
        }
        return Ok(());
    }

    match code {
        KeyCode::Char('?') => app.toggle_help_overlay(),
        _ if matches_any(app.keybindings(), &code, |bindings| &bindings.quit) => app.request_quit(),
        KeyCode::Esc => {
            app.go_back();
        }
        _ if matches_any(app.keybindings(), &code, |bindings| &bindings.panel_focus) => {
            app.cycle_panel_focus()
        }
        KeyCode::Backspace => app.clear_view_scope(),
        _ if matches_any(app.keybindings(), &code, |bindings| &bindings.actions) => {
            if !app
                .perform_command_center_primary_action(CommandCenterPrimaryTrigger::Actions)
                .await?
            {
                app.open_action_menu()
            }
        }
        _ if matches_any(app.keybindings(), &code, |bindings| &bindings.action_layout) => {
            app.cycle_layout_preset()
        }
        _ if matches_any(app.keybindings(), &code, |bindings| &bindings.search) => {
            app.begin_search()
        }
        _ if matches_any(app.keybindings(), &code, |bindings| &bindings.command) => {
            if !app
                .perform_command_center_primary_action(CommandCenterPrimaryTrigger::Command)
                .await?
            {
                app.begin_command_input()
            }
        }
        _ if matches_any(app.keybindings(), &code, |bindings| &bindings.refresh) => {
            app.refresh().await?
        }
        _ if app.command_shortcuts_are_visible()
            && matches_any(app.keybindings(), &code, |bindings| &bindings.repeat_last) =>
        {
            app.repeat_last_command().await?
        }
        _ if app.command_shortcuts_are_visible()
            && matches_any(app.keybindings(), &code, |bindings| &bindings.macro_assign) =>
        {
            app.begin_macro_assign()
        }
        _ if matches_any(app.keybindings(), &code, |bindings| &bindings.move_down) => {
            app.select_next_pane()
        }
        _ if matches_any(app.keybindings(), &code, |bindings| &bindings.move_up) => {
            app.select_previous_pane()
        }
        KeyCode::PageUp => {
            app.scroll_details_page_older();
        }
        KeyCode::PageDown => {
            app.scroll_details_page_newer();
        }
        KeyCode::Home => {
            app.scroll_details_to_oldest();
        }
        KeyCode::End => {
            app.scroll_details_to_newest();
        }
        _ if matches_any(app.keybindings(), &code, |bindings| &bindings.mark) => {
            app.toggle_selected_mark()
        }
        _ if matches_any(app.keybindings(), &code, |bindings| &bindings.summaries) => {
            app.request_target_summaries().await?
        }
        _ if matches_any(app.keybindings(), &code, |bindings| &bindings.clear_marks) => {
            app.clear_marked_panes()
        }
        _ if matches_any(app.keybindings(), &code, |bindings| &bindings.smart_action) => {
            if !app
                .perform_command_center_primary_action(CommandCenterPrimaryTrigger::Smart)
                .await?
            {
                app.perform_smart_action().await?
            }
        }
        _ if matches_any(app.keybindings(), &code, |bindings| &bindings.focus) => {
            if !app
                .perform_command_center_primary_action(CommandCenterPrimaryTrigger::Focus)
                .await?
            {
                app.focus_selected_pane().await?
            }
        }
        _ if matches_any(app.keybindings(), &code, |bindings| &bindings.jump) => {
            if !app
                .perform_command_center_primary_action(CommandCenterPrimaryTrigger::Jump)
                .await?
            {
                app.jump_to_selected_pane().await?
            }
        }
        _ => {
            if app.command_shortcuts_are_visible()
                && let Some(token) = key_token(&code)
                && let Some(slot) = app.macro_slot_for_key_token(&token)
            {
                app.run_macro_slot(slot).await?;
            }
        }
    }

    Ok(())
}

#[cfg(test)]
fn draw(frame: &mut Frame, app: &App) {
    draw_with_profile(frame, app, TerminalProfile::modern());
}

fn draw_with_profile(frame: &mut Frame, app: &App, profile: TerminalProfile) {
    let theme = Theme::from_settings_with_profile(app.ui_settings(), profile);
    let area = frame.area();
    let layout = match app.layout_preset() {
        crate::app::LayoutPreset::Auto
        | crate::app::LayoutPreset::Horizontal
        | crate::app::LayoutPreset::Vertical => LayoutProfile {
            header_height: 1,
            footer_height: 1,
            board_percent: 55,
            context_percent: 45,
            stacked_board_percent: 52,
            stacked_context_percent: 48,
        },
    };
    let [header, body, footer_area] = Layout::vertical([
        Constraint::Length(layout.header_height),
        Constraint::Min(8),
        Constraint::Length(layout.footer_height),
    ])
    .areas(area);
    let header_width = header.width;
    let footer_width = footer_area.width;

    let header_summary = app.header_context_line_for_width(header_width.saturating_sub(12));
    let summary_width = header_summary.chars().count() as u16;
    if header_summary.is_empty() || summary_width.saturating_add(12) >= header.width {
        let title = Paragraph::new(Line::from(vec![
            Span::styled(app.title(), theme.brand_style()),
            Span::styled(
                if header_summary.is_empty() {
                    String::new()
                } else {
                    format!("  {header_summary}")
                },
                theme.body_style(),
            ),
        ]));
        frame.render_widget(title, header);
    } else {
        let [header_left, header_right] =
            Layout::horizontal([Constraint::Min(12), Constraint::Length(summary_width)])
                .areas(header);
        let title = Paragraph::new(Line::from(vec![Span::styled(
            app.title(),
            theme.brand_style(),
        )]));
        let summary = Paragraph::new(header_summary)
            .style(theme.body_style())
            .alignment(Alignment::Right);
        frame.render_widget(title, header_left);
        frame.render_widget(summary, header_right);
    }

    if app.is_help_overlay_active() {
        let footer =
            Paragraph::new(app.footer_line_for_width(footer_width)).style(theme.body_style());
        frame.render_widget(footer, footer_area);
        draw_help_overlay(frame, app, theme, body);
        return;
    }

    if let Some((title, lines)) = app.overlay_panel() {
        let area = overlay_rect(body, &title, &lines);
        let prepared = prepare_overlay_lines_with_scroll(
            &title,
            lines,
            area.width.saturating_sub(4),
            area.height,
            app.details_scroll_offset(),
        );
        if title == "Output" {
            app.observe_details_scroll_viewport(prepared.scroll_metrics);
        }
        let footer =
            Paragraph::new(app.footer_line_for_width(footer_width)).style(theme.body_style());
        frame.render_widget(footer, footer_area);
        draw_shell_overlay(frame, theme, body, &title, prepared, area);
        return;
    }

    let body_layout = ResolvedBodyLayout::for_app(app, body, layout);
    let split_gutter = if body.width < 72 || body.height < 12 {
        0
    } else {
        1
    };
    let stack_gutter = if body.height <= 12 { 0 } else { 1 };
    let [board_area, inspector_area] = match body_layout.mode {
        BodyLayoutMode::SplitColumns => {
            let [board, _, inspector] = Layout::horizontal([
                Constraint::Percentage(body_layout.board_percent),
                Constraint::Length(split_gutter),
                Constraint::Percentage(body_layout.context_percent),
            ])
            .areas(body);
            [board, inspector]
        }
        BodyLayoutMode::Stack => {
            let [board, _, inspector] = Layout::vertical([
                Constraint::Percentage(body_layout.board_percent),
                Constraint::Length(stack_gutter),
                Constraint::Percentage(body_layout.context_percent),
            ])
            .areas(body);
            [board, inspector]
        }
    };

    let board_mode = BoardLayoutMode::for_width(board_area.width);

    let board_row_capacity = board_area.height.saturating_sub(3);
    let (board_row_limit, board_rows, board_location_width, latest_width) =
        board_rows_for_capacity(app, board_row_capacity, board_mode, board_area.width);
    let rows = board_rows.into_iter().map(|row| {
        let style = match row.tone() {
            BoardRowTone::Default => theme.default_row_style(),
            BoardRowTone::Subdued => theme.subdued_row_style(),
            BoardRowTone::Selected => theme.selected_row_style(),
            BoardRowTone::Staged => theme.confirm_row_style(),
            BoardRowTone::Targeted => theme.targeted_row_style(),
            BoardRowTone::Attention => theme.attention_row_style(),
            BoardRowTone::Watching => theme.watching_row_style(),
            BoardRowTone::Alert => theme.alert_row_style(),
        };
        Row::new(board_cells_for_width(
            &row,
            board_mode,
            board_location_width,
            latest_width,
            theme,
        ))
        .height(board_row_height(&row, board_mode, latest_width))
        .style(style)
    });
    let board_title = app.board_title_for_width(board_row_limit, board_area.width);
    let board_block = if app.should_emphasize_fleet_panel() {
        theme.focused_block(&board_title)
    } else {
        theme.block(&board_title)
    };
    let board = Table::new(rows, board_constraints(board_mode, board_location_width))
        .header(Row::new(board_headers(board_mode)).style(theme.table_header_style()))
        .column_spacing(1)
        .block(board_block);
    frame.render_widget(board, board_area);

    let inspector_title = app.inspector_title();
    let prepared_inspector = prepare_context_panel_lines_with_scroll(
        &inspector_title,
        app.inspector_lines(),
        inspector_area.width.saturating_sub(2),
        inspector_area.height.saturating_sub(2),
        app.details_scroll_offset(),
    );
    if app.overlay_panel().is_none() && !app.is_help_overlay_active() {
        app.observe_details_scroll_viewport(prepared_inspector.scroll_metrics);
    }
    let inspector_scroll = prepared_inspector
        .scroll
        .filter(|_| app.should_emphasize_context_panel());
    let inspector = style_panel_lines(theme, prepared_inspector.lines);
    let inspector_block = if app.should_emphasize_context_panel() {
        theme.focused_block(&inspector_title)
    } else {
        theme.block(&inspector_title)
    };
    let inspector = Paragraph::new(inspector).block(inspector_block);
    frame.render_widget(inspector, inspector_area);
    render_scroll_indicator(frame, theme, inspector_area, inspector_scroll);

    let footer = Paragraph::new(app.footer_line_for_width(footer_width)).style(theme.body_style());
    frame.render_widget(footer, footer_area);
}

#[derive(Clone, Copy)]
struct LayoutProfile {
    header_height: u16,
    footer_height: u16,
    board_percent: u16,
    context_percent: u16,
    stacked_board_percent: u16,
    stacked_context_percent: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OverlayKind {
    Generic,
    Browse,
    CommandCenter,
    Actions,
    Output,
    Help,
}

fn board_headers(mode: BoardLayoutMode) -> Vec<&'static str> {
    match mode {
        BoardLayoutMode::Full => vec!["", "Where", "Tool", "Now", "Latest"],
        BoardLayoutMode::Standard => vec!["", "Where", "Now", "Latest"],
        BoardLayoutMode::Compact => vec!["", "Where", "Latest"],
    }
}

fn board_constraints(mode: BoardLayoutMode, location_width: u16) -> Vec<Constraint> {
    match mode {
        BoardLayoutMode::Full => vec![
            Constraint::Length(2),
            Constraint::Length(location_width),
            Constraint::Max(8),
            Constraint::Max(9),
            Constraint::Fill(1),
        ],
        BoardLayoutMode::Standard => vec![
            Constraint::Length(2),
            Constraint::Length(location_width),
            Constraint::Max(9),
            Constraint::Fill(1),
        ],
        BoardLayoutMode::Compact => vec![
            Constraint::Length(2),
            Constraint::Length(location_width),
            Constraint::Fill(1),
        ],
    }
}

fn board_location_width(rows: &[crate::app::BoardRow]) -> u16 {
    rows.iter()
        .map(|row| row.location.chars().count() as u16)
        .max()
        .unwrap_or(BOARD_LOCATION_MIN_WIDTH)
        .clamp(BOARD_LOCATION_MIN_WIDTH, BOARD_LOCATION_MAX_WIDTH)
}

fn board_rows_for_capacity(
    app: &App,
    capacity: u16,
    mode: BoardLayoutMode,
    area_width: u16,
) -> (usize, Vec<crate::app::BoardRow>, u16, u16) {
    if capacity == 0 {
        let latest_width = board_latest_width(area_width, mode, BOARD_LOCATION_MIN_WIDTH);
        return (0, Vec::new(), BOARD_LOCATION_MIN_WIDTH, latest_width);
    }

    let preview_rows = app.board_rows(usize::from(capacity));
    let location_width = board_location_width(&preview_rows);
    let latest_width = board_latest_width(area_width, mode, location_width);
    let selected_extra = preview_rows
        .iter()
        .find(|row| row.selected)
        .map(|row| board_row_height(row, mode, latest_width).saturating_sub(1))
        .unwrap_or(0);

    let limit = usize::from(capacity.saturating_sub(selected_extra).max(1));
    if limit == usize::from(capacity) {
        (limit, preview_rows, location_width, latest_width)
    } else {
        (limit, app.board_rows(limit), location_width, latest_width)
    }
}

fn board_latest_width(area_width: u16, mode: BoardLayoutMode, location_width: u16) -> u16 {
    let inner_width = area_width.saturating_sub(2);
    let column_spacing = board_headers(mode).len().saturating_sub(1) as u16;
    let fixed_width = match mode {
        BoardLayoutMode::Full => 2 + location_width + 8 + 9,
        BoardLayoutMode::Standard => 2 + location_width + 9,
        BoardLayoutMode::Compact => 2 + location_width,
    };

    inner_width
        .saturating_sub(column_spacing)
        .saturating_sub(fixed_width)
        .max(1)
}

#[cfg(test)]
fn board_cells(row: &crate::app::BoardRow, mode: BoardLayoutMode) -> Vec<Cell<'static>> {
    match mode {
        BoardLayoutMode::Full => vec![
            Cell::from(row.flags()),
            Cell::from(truncate_location_cell(
                &row.location,
                usize::from(BOARD_LOCATION_MIN_WIDTH),
            )),
            Cell::from(truncate_cell(&row.command, 8)),
            Cell::from(row.lifecycle.clone()),
            Cell::from(row.title.clone()),
        ],
        BoardLayoutMode::Standard => vec![
            Cell::from(row.flags()),
            Cell::from(truncate_location_cell(
                &row.location,
                usize::from(BOARD_LOCATION_MIN_WIDTH),
            )),
            Cell::from(row.lifecycle.clone()),
            Cell::from(row.standard_latest()),
        ],
        BoardLayoutMode::Compact => vec![
            Cell::from(row.flags()),
            Cell::from(truncate_location_cell(
                &row.location,
                usize::from(BOARD_LOCATION_MIN_WIDTH),
            )),
            Cell::from(row.compact_latest()),
        ],
    }
}

fn board_cells_for_width(
    row: &crate::app::BoardRow,
    mode: BoardLayoutMode,
    location_width: u16,
    latest_width: u16,
    theme: Theme,
) -> Vec<Cell<'static>> {
    match mode {
        BoardLayoutMode::Full => vec![
            Cell::from(row.flags()).style(board_marker_style(theme, row)),
            Cell::from(truncate_location_cell(
                &row.location,
                usize::from(location_width),
            )),
            Cell::from(truncate_cell(&row.command, 8)),
            Cell::from(row.lifecycle.clone()).style(board_state_style(theme, row)),
            board_latest_cell(theme, row, row.title.clone(), latest_width),
        ],
        BoardLayoutMode::Standard => vec![
            Cell::from(row.flags()).style(board_marker_style(theme, row)),
            Cell::from(truncate_location_cell(
                &row.location,
                usize::from(location_width),
            )),
            Cell::from(row.lifecycle.clone()).style(board_state_style(theme, row)),
            board_latest_cell(theme, row, row.standard_latest(), latest_width),
        ],
        BoardLayoutMode::Compact => vec![
            Cell::from(row.flags()).style(board_marker_style(theme, row)),
            Cell::from(truncate_location_cell(
                &row.location,
                usize::from(location_width),
            )),
            board_latest_cell(theme, row, row.compact_latest(), latest_width),
        ],
    }
}

fn board_marker_style(theme: Theme, row: &crate::app::BoardRow) -> Style {
    if row.staged {
        return theme.confirm_row_style();
    }
    if row.attention == "!" && row.status == "waiting" {
        return theme.attention_row_style();
    }
    if row.attention == "!" {
        return theme.alert_row_style();
    }
    if row.attention == "~" {
        return theme.watching_row_style();
    }
    if row.targeted || row.marked {
        return theme.targeted_row_style();
    }
    if row.selected {
        return theme.selected_row_style();
    }

    theme.default_row_style()
}

fn board_state_style(theme: Theme, row: &crate::app::BoardRow) -> Style {
    match row.lifecycle.as_str() {
        "needs you" => theme.attention_row_style(),
        "failed" | "stale" => theme.alert_row_style(),
        "watching" => theme.watching_row_style(),
        "done" => theme.success_style(),
        "quiet" | "checking" | "muted" => theme.subdued_row_style(),
        _ => theme.default_row_style(),
    }
}

fn board_latest_style(theme: Theme, row: &crate::app::BoardRow) -> Style {
    if row.attention == "!" && row.status == "waiting" {
        theme.attention_row_style()
    } else if row.attention == "!" {
        theme.alert_row_style()
    } else if row.attention == "~" {
        theme.watching_row_style()
    } else {
        Style::default()
    }
}

fn board_row_height(row: &crate::app::BoardRow, mode: BoardLayoutMode, latest_width: u16) -> u16 {
    selected_board_latest_lines(row, board_latest_text(row, mode), latest_width).len() as u16
}

fn board_latest_cell(
    theme: Theme,
    row: &crate::app::BoardRow,
    latest: String,
    latest_width: u16,
) -> Cell<'static> {
    let lines = selected_board_latest_lines(row, latest, latest_width);
    let style = board_latest_style(theme, row);
    if lines.len() <= 1 {
        Cell::from(lines.into_iter().next().unwrap_or_default()).style(style)
    } else {
        Cell::from(Text::from(
            lines.into_iter().map(Line::from).collect::<Vec<_>>(),
        ))
        .style(style)
    }
}

fn board_latest_text(row: &crate::app::BoardRow, mode: BoardLayoutMode) -> String {
    match mode {
        BoardLayoutMode::Full => row.title.clone(),
        BoardLayoutMode::Standard => row.standard_latest(),
        BoardLayoutMode::Compact => row.compact_latest(),
    }
}

fn selected_board_latest_lines(
    row: &crate::app::BoardRow,
    latest: String,
    latest_width: u16,
) -> Vec<String> {
    if !row.selected || !should_wrap_selected_latest(row) || latest_width == 0 {
        return vec![latest];
    }

    let max_lines = 3;
    let mut wrapped = wrap_words(&latest, usize::from(latest_width));
    if wrapped.len() > max_lines {
        let mut visible = wrapped
            .drain(..max_lines.saturating_sub(1))
            .collect::<Vec<_>>();
        let remainder = wrapped.join(" ");
        visible.push(append_cell_ellipsis(&remainder, usize::from(latest_width)));
        wrapped = visible;
    }

    wrapped
}

fn should_wrap_selected_latest(row: &crate::app::BoardRow) -> bool {
    matches!(
        row.status.as_str(),
        "running" | "waiting" | "error" | "stuck"
    )
}

fn append_cell_ellipsis(text: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    if width <= 3 {
        return truncate_cell(text, width);
    }

    let mut base = text
        .chars()
        .take(width.saturating_sub(3))
        .collect::<String>();
    let trimmed_len = base.trim_end().len();
    base.truncate(trimmed_len);
    base.push_str("...");
    base
}

fn truncate_cell(text: &str, max_chars: usize) -> String {
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

fn truncate_location_cell(text: &str, max_chars: usize) -> String {
    if max_chars == 0 || text.chars().count() <= max_chars {
        return text.to_owned();
    }

    let Some((prefix, suffix)) = text.rsplit_once('#') else {
        return truncate_cell(text, max_chars);
    };
    let suffix = format!("#{suffix}");
    let suffix_len = suffix.chars().count();

    if suffix_len >= max_chars {
        return truncate_cell(text, max_chars);
    }

    let prefix_budget = max_chars.saturating_sub(suffix_len);
    if prefix_budget <= 3 {
        return truncate_cell(text, max_chars);
    }

    let mut shortened = prefix.chars().take(prefix_budget - 3).collect::<String>();
    shortened.push_str("...");
    shortened.push_str(&suffix);
    shortened
}

fn draw_shell_overlay(
    frame: &mut Frame,
    theme: Theme,
    body: Rect,
    title: &str,
    prepared: PreparedPanelLines,
    area: Rect,
) {
    let content = style_panel_lines(theme, prepared.lines);
    let overlay =
        Paragraph::new(content).block(theme.focused_block(title).padding(Padding::horizontal(1)));
    frame.render_widget(Clear, body);
    frame.render_widget(overlay, area);
    render_scroll_indicator(frame, theme, area, prepared.scroll);
}

fn draw_help_overlay(frame: &mut Frame, app: &App, theme: Theme, body: Rect) {
    let lines = app.help_lines();
    let area = overlay_rect(body, &app.help_overlay_title(), &lines);
    frame.render_widget(Clear, body);
    let lines = style_panel_lines(theme, fit_panel_lines(lines, area.width.saturating_sub(4)));
    let help = Paragraph::new(lines).block(
        theme
            .focused_block(&app.help_overlay_title())
            .padding(Padding::horizontal(1)),
    );
    frame.render_widget(help, area);
}

fn render_scroll_indicator(
    frame: &mut Frame,
    theme: Theme,
    area: Rect,
    scroll: Option<ScrollIndicator>,
) {
    let Some(scroll) = scroll else {
        return;
    };
    if scroll.content_len <= scroll.viewport_len || area.width < 3 {
        return;
    }

    let inner_height = usize::from(area.height.saturating_sub(2));
    if scroll.track_start_row >= inner_height {
        return;
    }

    let track_len = scroll
        .track_len
        .min(inner_height.saturating_sub(scroll.track_start_row));
    let Some(geometry) = ScrollbarGeometry::new(
        scroll.content_len,
        scroll.viewport_len,
        scroll.position_from_top,
        track_len,
    ) else {
        return;
    };

    let x = area.x.saturating_add(area.width.saturating_sub(2));
    let y = area
        .y
        .saturating_add(1)
        .saturating_add(scroll.track_start_row as u16);
    let (track_symbol, thumb_symbol) = if theme.ascii_borders {
        (".", "#")
    } else {
        ("░", "█")
    };
    let buffer = frame.buffer_mut();
    for row in 0..geometry.track_len {
        let in_thumb =
            row >= geometry.thumb_start && row < geometry.thumb_start + geometry.thumb_len;
        let (symbol, style) = if in_thumb {
            (thumb_symbol, theme.accent_style())
        } else {
            (track_symbol, theme.surface_style())
        };
        buffer.set_string(x, y.saturating_add(row as u16), symbol, style);
    }
}

fn fit_panel_lines(lines: Vec<String>, width: u16) -> Vec<String> {
    let source_len = lines.len();
    let source_hash = panel_lines_hash(width, &lines);
    if let Some(cached) = FITTED_PANEL_LINES_CACHE.with(|cache| {
        let cached = cache.borrow();
        cached
            .as_ref()
            .filter(|cached| {
                cached.width == width
                    && cached.source_hash == source_hash
                    && cached.source_len == source_len
            })
            .map(|cached| cached.fitted.clone())
    }) {
        return cached;
    }

    let fitted = fit_panel_lines_uncached(lines, width);
    FITTED_PANEL_LINES_CACHE.with(|cache| {
        *cache.borrow_mut() = Some(FittedPanelLinesCache {
            width,
            source_hash,
            source_len,
            fitted: fitted.clone(),
        });
    });
    fitted
}

fn fit_panel_lines_uncached(lines: Vec<String>, width: u16) -> Vec<String> {
    lines
        .into_iter()
        .flat_map(|line| fit_panel_line(&line, width))
        .collect()
}

fn panel_lines_hash(width: u16, lines: &[String]) -> u64 {
    let mut hasher = DefaultHasher::new();
    width.hash(&mut hasher);
    lines.len().hash(&mut hasher);
    for line in lines {
        line.hash(&mut hasher);
    }
    hasher.finish()
}

fn fit_panel_line(line: &str, width: u16) -> Vec<String> {
    let mut wrapped = wrap_panel_line(line, width);
    let max_lines = max_wrapped_panel_lines(line);
    if wrapped.len() > max_lines {
        wrapped.truncate(max_lines);
    }
    wrapped
}

fn max_wrapped_panel_lines(line: &str) -> usize {
    if line.starts_with("Blocked: ")
        || line.starts_with("Problem: ")
        || line.starts_with("Action: ")
        || line.starts_with("Now: ")
        || line.starts_with("Mission: ")
        || line.starts_with("Selected: ")
        || line.starts_with("Target: ")
        || line.starts_with("Send: ")
    {
        2
    } else {
        usize::MAX
    }
}

fn wrap_panel_line(line: &str, width: u16) -> Vec<String> {
    let width = usize::from(width);
    if width == 0 || line.is_empty() {
        return vec![String::new()];
    }
    if line.chars().count() <= width {
        return vec![line.to_owned()];
    }
    if is_section_heading(line) {
        return vec![truncate_panel_line(line, width as u16)];
    }
    if let Some((label, value)) = split_label_value(line) {
        return wrap_prefixed_panel_value(&label, &value, width);
    }
    if let Some(value) = line.strip_prefix("  ") {
        return wrap_prefixed_panel_value("  ", value, width);
    }
    wrap_words(line, width)
}

fn wrap_prefixed_panel_value(prefix: &str, value: &str, width: usize) -> Vec<String> {
    let prefix_len = prefix.chars().count();
    if prefix_len >= width {
        return wrap_words(&format!("{prefix}{value}"), width);
    }

    let continuation = " ".repeat(prefix_len.min(width.saturating_sub(1)));
    let first_width = width.saturating_sub(prefix_len).max(1);
    let continuation_width = width.saturating_sub(continuation.chars().count()).max(1);
    let wrapped = wrap_words_with_widths(value, first_width, continuation_width);

    if wrapped.is_empty() {
        return vec![prefix.to_owned()];
    }

    wrapped
        .into_iter()
        .enumerate()
        .map(|(index, chunk)| {
            if index == 0 {
                format!("{prefix}{chunk}")
            } else {
                format!("{continuation}{chunk}")
            }
        })
        .collect()
}

fn wrap_words(text: &str, width: usize) -> Vec<String> {
    wrap_words_with_widths(text, width, width)
}

fn wrap_words_with_widths(
    text: &str,
    first_width: usize,
    continuation_width: usize,
) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut current_len = 0;
    let mut current_width = first_width.max(1);

    for word in text.split_whitespace() {
        push_wrapped_word(
            &mut lines,
            &mut current,
            &mut current_len,
            &mut current_width,
            word,
            first_width.max(1),
            continuation_width.max(1),
        );
    }

    if current_len > 0 {
        lines.push(current);
    }

    if lines.is_empty() {
        vec![String::new()]
    } else {
        lines
    }
}

fn push_wrapped_word(
    lines: &mut Vec<String>,
    current: &mut String,
    current_len: &mut usize,
    current_width: &mut usize,
    word: &str,
    first_width: usize,
    continuation_width: usize,
) {
    let word_len = word.chars().count();
    let separator = usize::from(*current_len > 0);

    if *current_len + separator + word_len <= *current_width {
        if *current_len > 0 {
            current.push(' ');
        }
        current.push_str(word);
        *current_len += separator + word_len;
        return;
    }

    if *current_len > 0 {
        lines.push(std::mem::take(current));
        *current_len = 0;
        *current_width = continuation_width.max(1);
    }

    let mut rest = word;
    let mut rest_len = word_len;
    while rest_len > *current_width {
        let (chunk, remaining) = split_at_char_width(rest, *current_width);
        lines.push(chunk.to_owned());
        rest = remaining;
        rest_len = rest.chars().count();
        *current_width = continuation_width.max(1);
    }

    current.push_str(rest);
    *current_len = rest_len;
    if lines.is_empty() {
        *current_width = first_width.max(1);
    }
}

fn split_at_char_width(text: &str, width: usize) -> (&str, &str) {
    if width == 0 {
        return ("", text);
    }
    let split = text
        .char_indices()
        .nth(width)
        .map(|(index, _)| index)
        .unwrap_or(text.len());
    text.split_at(split)
}

#[derive(Debug, Clone)]
struct PreparedPanelLines {
    lines: Vec<String>,
    scroll_metrics: Option<ScrollMetrics>,
    scroll: Option<ScrollIndicator>,
}

impl PreparedPanelLines {
    fn without_scroll(lines: Vec<String>) -> Self {
        Self {
            lines,
            scroll_metrics: None,
            scroll: None,
        }
    }

    fn with_scroll_metrics(lines: Vec<String>, metrics: ScrollMetrics) -> Self {
        Self {
            lines,
            scroll_metrics: Some(metrics),
            scroll: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ScrollMetrics {
    pub(crate) content_len: usize,
    pub(crate) viewport_len: usize,
}

impl ScrollMetrics {
    pub(crate) fn max_offset(self) -> usize {
        if self.viewport_len == 0 {
            0
        } else {
            self.content_len.saturating_sub(self.viewport_len)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ScrollIndicator {
    content_len: usize,
    viewport_len: usize,
    position_from_top: usize,
    track_start_row: usize,
    track_len: usize,
}

impl ScrollIndicator {
    fn at_track(self, track_start_row: usize, track_len: usize) -> Self {
        Self {
            track_start_row,
            track_len,
            ..self
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ScrollbarGeometry {
    track_len: usize,
    thumb_start: usize,
    thumb_len: usize,
}

impl ScrollbarGeometry {
    fn new(
        content_len: usize,
        viewport_len: usize,
        position_from_top: usize,
        track_len: usize,
    ) -> Option<Self> {
        if content_len <= viewport_len || viewport_len == 0 || track_len < 2 {
            return None;
        }

        let max_offset = content_len.saturating_sub(viewport_len);
        if max_offset == 0 {
            return None;
        }

        let thumb_len = track_len
            .saturating_mul(viewport_len)
            .checked_div(content_len)
            .unwrap_or(0)
            .clamp(1, track_len.saturating_sub(1));
        let travel = track_len.saturating_sub(thumb_len);
        let position = position_from_top.min(max_offset);
        let thumb_start = position
            .saturating_mul(travel)
            .saturating_add(max_offset / 2)
            / max_offset;

        Some(Self {
            track_len,
            thumb_start,
            thumb_len,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ScrollViewport {
    start: usize,
    end: usize,
    height: usize,
}

impl ScrollViewport {
    fn new(total: usize, height: usize, offset_from_bottom: usize) -> Self {
        let height = height.min(total);
        let max_offset = total.saturating_sub(height);
        let offset = offset_from_bottom.min(max_offset);
        let end = total.saturating_sub(offset);
        let start = end.saturating_sub(height);
        Self { start, end, height }
    }

    fn indicator(self, total: usize) -> Option<ScrollIndicator> {
        (total > self.height).then_some(ScrollIndicator {
            content_len: total,
            viewport_len: self.height,
            position_from_top: self.start,
            track_start_row: 0,
            track_len: self.height,
        })
    }

    fn metrics(self, total: usize) -> ScrollMetrics {
        ScrollMetrics {
            content_len: total,
            viewport_len: self.height,
        }
    }
}

#[cfg(test)]
fn prepare_context_panel_lines(
    title: &str,
    lines: Vec<String>,
    width: u16,
    height: u16,
) -> Vec<String> {
    prepare_context_panel_lines_with_scroll(title, lines, width, height, 0).lines
}

fn prepare_context_panel_lines_with_scroll(
    title: &str,
    lines: Vec<String>,
    width: u16,
    height: u16,
    offset_from_bottom: usize,
) -> PreparedPanelLines {
    let fitted = fit_panel_lines(lines, width);
    prioritize_context_panel_lines_with_scroll(title, fitted, height as usize, offset_from_bottom)
}

#[cfg(test)]
fn prioritize_context_panel_lines(title: &str, lines: Vec<String>, max_rows: usize) -> Vec<String> {
    prioritize_context_panel_lines_with_scroll(title, lines, max_rows, 0).lines
}

fn prioritize_context_panel_lines_with_scroll(
    title: &str,
    lines: Vec<String>,
    max_rows: usize,
    offset_from_bottom: usize,
) -> PreparedPanelLines {
    if title == "Details" {
        prioritize_selected_panel_lines(lines, max_rows, offset_from_bottom)
    } else {
        PreparedPanelLines::without_scroll(lines)
    }
}

#[cfg(test)]
fn prepare_overlay_lines(title: &str, lines: Vec<String>, width: u16, height: u16) -> Vec<String> {
    prepare_overlay_lines_with_scroll(title, lines, width, height, 0).lines
}

fn prepare_overlay_lines_with_scroll(
    title: &str,
    lines: Vec<String>,
    width: u16,
    height: u16,
    offset_from_bottom: usize,
) -> PreparedPanelLines {
    let fitted = fit_panel_lines(lines, width);
    prioritize_overlay_lines_with_scroll(
        title,
        fitted,
        height.saturating_sub(2) as usize,
        offset_from_bottom,
    )
}

fn prioritize_overlay_lines_with_scroll(
    title: &str,
    lines: Vec<String>,
    max_rows: usize,
    offset_from_bottom: usize,
) -> PreparedPanelLines {
    if title == "More" {
        PreparedPanelLines::without_scroll(prioritize_actions_overlay_lines(lines, max_rows))
    } else if title == "Command Center" {
        PreparedPanelLines::without_scroll(prioritize_overview_overlay_lines(lines, max_rows))
    } else if title == "Send" || title == "Reply" {
        PreparedPanelLines::without_scroll(prioritize_send_overlay_lines(lines, max_rows))
    } else if title == "Output" {
        prioritize_output_overlay_lines_with_scroll(lines, max_rows, offset_from_bottom)
    } else if title == "Fleets" {
        PreparedPanelLines::without_scroll(prioritize_fleet_picker_lines(lines, max_rows))
    } else {
        PreparedPanelLines::without_scroll(lines)
    }
}

fn prioritize_fleet_picker_lines(lines: Vec<String>, max_rows: usize) -> Vec<String> {
    if lines.len() <= max_rows || max_rows == 0 {
        return lines;
    }

    let selected = lines
        .iter()
        .position(|line| line.starts_with('>'))
        .unwrap_or(0);
    let mut start = selected.saturating_sub(max_rows / 2);
    if start + max_rows > lines.len() {
        start = lines.len().saturating_sub(max_rows);
    }
    lines.into_iter().skip(start).take(max_rows).collect()
}

fn prioritize_selected_panel_lines(
    lines: Vec<String>,
    max_rows: usize,
    offset_from_bottom: usize,
) -> PreparedPanelLines {
    if lines.len() <= max_rows || max_rows == 0 {
        return match section_scroll_metrics(
            &lines,
            "Output",
            &["Agent report", "Output", "Command"],
        ) {
            Some(metrics) => PreparedPanelLines::with_scroll_metrics(lines, metrics),
            None => PreparedPanelLines::without_scroll(lines),
        };
    }

    let mut prelude = Vec::new();
    let mut metadata = Vec::new();
    let mut sections = Vec::<(String, Vec<String>)>::new();
    let mut current_section = None::<usize>;

    for line in lines {
        if matches!(line.as_str(), "Agent report" | "Output" | "Command") {
            sections.push((line, Vec::new()));
            current_section = Some(sections.len() - 1);
            continue;
        }

        if line.is_empty() {
            continue;
        }

        if current_section.is_some() && is_selected_metadata_line(&line) {
            metadata.push(line);
        } else if let Some(index) = current_section {
            sections[index].1.push(line);
        } else {
            prelude.push(line);
        }
    }

    let compact = max_rows <= 10;
    let mut prioritized = prelude
        .into_iter()
        .take(max_rows.min(5))
        .collect::<Vec<_>>();
    let mut prepared_scroll_metrics = None;

    for (heading, items) in sections {
        let separator = usize::from(!compact && !prioritized.is_empty());
        if prioritized.len().saturating_add(separator + 2) > max_rows {
            continue;
        }
        if separator == 1 {
            prioritized.push(String::new());
        }
        prioritized.push(heading.clone());

        let available = max_rows.saturating_sub(prioritized.len());
        let cap = selected_section_cap(&heading, compact, available);
        let (visible, section_metrics, section_scroll) =
            prioritize_selected_section_items(&heading, items, cap, offset_from_bottom);
        let track_start = prioritized.len();
        let track_len = visible.len();
        if visible.is_empty() {
            prioritized.pop();
            if separator == 1 {
                prioritized.pop();
            }
            continue;
        }
        let scroll = section_scroll.map(|scroll| scroll.at_track(track_start, track_len));
        prioritized.extend(visible);
        if let Some(metrics) = section_metrics {
            prepared_scroll_metrics = Some(metrics);
        }
        if let Some(scroll) = scroll {
            return PreparedPanelLines {
                lines: prioritized,
                scroll_metrics: prepared_scroll_metrics,
                scroll: Some(scroll),
            };
        }
    }

    if !compact {
        for line in metadata {
            if prioritized.len() >= max_rows {
                break;
            }
            prioritized.push(line);
        }
    }

    PreparedPanelLines {
        lines: prioritized,
        scroll_metrics: prepared_scroll_metrics,
        scroll: None,
    }
}

fn section_scroll_metrics(
    lines: &[String],
    heading: &str,
    section_headings: &[&str],
) -> Option<ScrollMetrics> {
    let start = lines.iter().position(|line| line == heading)? + 1;
    let mut content_len = 0;
    for line in lines.iter().skip(start) {
        if section_headings.iter().any(|candidate| line == candidate) {
            break;
        }
        if !line.is_empty() {
            content_len += 1;
        }
    }

    (content_len > 0).then_some(ScrollMetrics {
        content_len,
        viewport_len: content_len,
    })
}

fn selected_section_cap(heading: &str, compact: bool, available: usize) -> usize {
    let desired = match heading {
        "Output" if compact => available.min(6),
        "Output" => available,
        "Command" if compact => 1,
        "Command" => 2,
        "Agent report" if compact => 1,
        "Agent report" => 2,
        _ if compact => 1,
        _ => 2,
    };
    desired.min(available)
}

fn is_selected_metadata_line(line: &str) -> bool {
    line.starts_with("Updated: ")
        || line.starts_with("Lane: ")
        || line.starts_with("Review: ")
        || line.starts_with("pane CPU/mem:")
}

fn prioritize_actions_overlay_lines(lines: Vec<String>, max_rows: usize) -> Vec<String> {
    if lines.len() <= max_rows || max_rows == 0 {
        return lines;
    }

    let mut prelude = Vec::new();
    let mut sections = std::collections::BTreeMap::<String, Vec<String>>::new();
    let mut current_section = None::<String>;

    for line in lines {
        match action_section_key(&line) {
            Some(section) => {
                current_section = Some(section.to_owned());
                sections.entry(section.to_owned()).or_default();
            }
            _ => {
                if let Some(section) = &current_section {
                    sections.entry(section.clone()).or_default().push(line);
                } else if !line.is_empty() {
                    prelude.push(line);
                }
            }
        }
    }

    let compact = max_rows <= 18;
    let has_reports = sections
        .get("reports")
        .is_some_and(|items| !items.is_empty());
    let has_settings = sections
        .get("settings")
        .is_some_and(|items| !items.is_empty());
    if max_rows <= 8 {
        return prioritize_tiny_actions_overlay_lines(prelude, &sections, max_rows);
    }
    let mut view_cap = if max_rows <= 10 {
        3
    } else if max_rows >= 24 {
        7
    } else {
        5
    };
    if max_rows <= 18 && has_reports && has_settings && view_cap > 4 {
        view_cap = 4;
    }
    let pane_cap = if max_rows <= 8 {
        1
    } else if max_rows >= 20 {
        4
    } else {
        3
    };
    let settings_cap = if max_rows >= 18 {
        4
    } else if max_rows >= 16 {
        3
    } else {
        1
    };
    let stale_target_recovery = prelude
        .iter()
        .any(|line| line.contains("has no live panes"));
    let active_send_scope = prelude
        .iter()
        .any(|line| line.starts_with("send list") || line.starts_with("send to fleet "));
    let active_send_scope = active_send_scope
        || prelude
            .iter()
            .any(|line| line.starts_with("To: the send list") || line.starts_with("To: fleet "));
    let active_send_list_cap = if max_rows >= 12 { 4 } else { 2 };
    let active_view_cap = if max_rows >= 20 {
        view_cap
    } else {
        view_cap.min(4)
    };
    let active_pane_cap = if max_rows >= 20 { 4 } else { pane_cap.min(3) };
    let mut prioritized = prelude.into_iter().take(2).collect::<Vec<_>>();
    let section_order = if stale_target_recovery {
        [
            ("send list", 2_usize),
            ("view", view_cap),
            ("start", 1_usize),
            ("pane", pane_cap),
            ("reports", 1_usize),
            ("settings", settings_cap),
        ]
    } else if active_send_scope {
        [
            ("send list", active_send_list_cap),
            ("view", active_view_cap),
            ("pane", active_pane_cap),
            ("start", 1_usize),
            ("reports", 1_usize),
            ("settings", settings_cap),
        ]
    } else if max_rows <= 8 {
        [
            ("start", 1_usize),
            ("view", view_cap),
            ("pane", pane_cap),
            ("send list", 2_usize),
            ("reports", 1_usize),
            ("settings", settings_cap),
        ]
    } else {
        [
            ("view", view_cap),
            ("send list", 2_usize),
            ("start", 1_usize),
            ("pane", pane_cap),
            ("reports", 1_usize),
            ("settings", settings_cap),
        ]
    };
    for (section, cap) in section_order {
        let Some(items) = sections.get(section) else {
            continue;
        };
        if items.is_empty() {
            continue;
        }

        let separator = usize::from(!compact && !prioritized.is_empty());
        let needed = separator + 2;
        if prioritized.len().saturating_add(needed) > max_rows {
            continue;
        }

        if separator == 1 {
            prioritized.push(String::new());
        }
        prioritized.push(action_section_label(section).to_owned());

        let available = max_rows.saturating_sub(prioritized.len());
        prioritized.extend(prioritize_action_section_items(
            section,
            items,
            cap.min(available),
        ));
    }

    if prioritized.is_empty() {
        Vec::new()
    } else {
        prioritized
    }
}

fn prioritize_tiny_actions_overlay_lines(
    prelude: Vec<String>,
    sections: &std::collections::BTreeMap<String, Vec<String>>,
    max_rows: usize,
) -> Vec<String> {
    let mut prioritized = prelude.into_iter().take(2).collect::<Vec<_>>();
    for key in recommended_action_keys(&prioritized) {
        if prioritized.len() >= max_rows {
            break;
        }
        if let Some(item) = first_action_item_for_key(sections, &key)
            && !prioritized.contains(&item)
        {
            prioritized.push(item);
        }
    }

    for (section, cap) in [
        ("start", 1_usize),
        ("view", 1_usize),
        ("pane", 1_usize),
        ("send list", 1_usize),
        ("settings", 1_usize),
        ("reports", 1_usize),
    ] {
        if prioritized.len() >= max_rows {
            break;
        }
        let Some(items) = sections.get(section) else {
            continue;
        };
        for item in prioritize_action_section_items(section, items, cap) {
            if prioritized.len() >= max_rows {
                break;
            }
            if !prioritized.contains(&item) {
                prioritized.push(item);
            }
        }
    }

    prioritized
}

fn recommended_action_keys(prelude: &[String]) -> Vec<String> {
    let recommendation = prelude
        .iter()
        .find_map(|line| line.split_once("Action: ").map(|(_, tail)| tail.trim()));
    let Some(recommendation) = recommendation else {
        return Vec::new();
    };

    recommendation
        .split(',')
        .filter_map(|part| {
            let part = part.trim().strip_prefix("or ").unwrap_or(part.trim());
            let key = part.split_whitespace().next()?.trim();
            (!key.is_empty()).then(|| key.to_owned())
        })
        .collect()
}

fn first_action_item_for_key(
    sections: &std::collections::BTreeMap<String, Vec<String>>,
    key: &str,
) -> Option<String> {
    for section in ["view", "start", "pane", "send list", "settings", "reports"] {
        let Some(items) = sections.get(section) else {
            continue;
        };
        if let Some(item) = items
            .iter()
            .find(|item| action_item_starts_with_key(item, key))
        {
            return Some(item.clone());
        }
    }
    None
}

fn action_section_key(line: &str) -> Option<&'static str> {
    match line {
        "start" | "Start" => Some("start"),
        "view" | "View" => Some("view"),
        "send list" | "Send List" => Some("send list"),
        "pane" | "Pane" => Some("pane"),
        "settings" | "Settings" => Some("settings"),
        "reports" | "Reports" => Some("reports"),
        _ => None,
    }
}

fn action_section_label(section: &str) -> &'static str {
    match section {
        "start" => "Start",
        "view" => "View",
        "send list" => "Send List",
        "pane" => "Pane",
        "settings" => "Settings",
        "reports" => "Reports",
        _ => "More",
    }
}

fn action_item_starts_with_key(item: &str, key: &str) -> bool {
    let trimmed = item.trim_start();
    let Some(rest) = trimmed.strip_prefix(key) else {
        return false;
    };
    rest.starts_with(' ')
}

fn prioritize_action_section_items(section: &str, items: &[String], cap: usize) -> Vec<String> {
    if cap == 0 {
        return Vec::new();
    }

    let section = action_section_key(section).unwrap_or(section);

    if section == "send list" {
        let priorities = [
            " clear send list",
            " save fleet",
            " choose fleet",
            " delete stale ",
            " delete ",
        ];
        let mut visible: Vec<String> = Vec::new();
        for needle in priorities {
            if visible.len() >= cap {
                break;
            }
            if let Some(item) = items
                .iter()
                .find(|item| item.contains(needle) && !visible.iter().any(|line| line == *item))
            {
                visible.push(item.clone());
            }
        }
        for item in items {
            if visible.len() >= cap {
                break;
            }
            if !visible.contains(item) {
                visible.push(item.clone());
            }
        }
        return visible;
    }

    if section == "view" {
        if cap == 1
            && let Some(item) = items
                .iter()
                .find(|item| item.contains("backspace show all panes"))
        {
            return vec![item.clone()];
        }

        if cap == 1
            && let Some(item) = items.iter().find(|item| item.contains(" command center"))
        {
            return vec![item.clone()];
        }

        let has_primary_view_action = items.iter().any(|item| {
            item.contains(" show output")
                || item.contains(" show details")
                || item.contains(" open window")
                || item.contains(" reply")
                || item.contains(" send text")
        });
        let priorities = if has_primary_view_action {
            [
                "backspace show all panes",
                " reply",
                " send text",
                " show output",
                " show details",
                " open window",
                " browse windows",
                " command center",
                " summarize panes",
                " refresh",
                " layout:",
                " sort by ",
                " show ",
            ]
        } else {
            [
                "backspace show all panes",
                " browse windows",
                " command center",
                " show output",
                " show details",
                " open window",
                " reply",
                " send text",
                " summarize panes",
                " refresh",
                " layout:",
                " sort by ",
                " show ",
            ]
        };
        let mut visible: Vec<String> = Vec::new();
        for needle in priorities {
            if visible.len() >= cap {
                break;
            }
            if let Some(item) = items
                .iter()
                .find(|item| item.contains(needle) && !visible.iter().any(|line| line == *item))
            {
                visible.push(item.clone());
            }
        }
        for item in items {
            if visible.len() >= cap {
                break;
            }
            if !visible.contains(item) {
                visible.push(item.clone());
            }
        }
        return visible;
    }

    if section == "settings" {
        let ssh_safe = items
            .iter()
            .any(|item| item.contains("desktop alerts unavailable on SSH"));
        let priorities = if cap >= 3 {
            [
                " pane CPU/mem",
                " desktop alerts",
                " terminal bell",
                " alert repeat delay",
                " alert types",
            ]
        } else if ssh_safe {
            [
                " desktop alerts",
                " terminal bell",
                " pane CPU/mem",
                " alert repeat delay",
                " alert types",
            ]
        } else {
            [
                " pane CPU/mem",
                " desktop alerts",
                " terminal bell",
                " alert repeat delay",
                " alert types",
            ]
        };
        let mut visible: Vec<String> = Vec::new();
        for needle in priorities {
            if visible.len() >= cap {
                break;
            }
            if let Some(item) = items
                .iter()
                .find(|item| item.contains(needle) && !visible.contains(item))
            {
                visible.push(item.to_owned());
            }
        }
        for item in items {
            if visible.len() >= cap {
                break;
            }
            if !visible.contains(item) {
                visible.push(item.to_owned());
            }
        }
        return visible;
    }

    if section != "pane" {
        return items.iter().take(cap).cloned().collect();
    }

    let mut visible = Vec::new();
    if items.iter().any(|item| item.contains(" answer yes"))
        && items.iter().any(|item| item.contains(" answer no"))
    {
        for needle in [
            " answer yes",
            " answer no",
            " mute alert",
            " unmute alert",
            " zoom pane",
        ] {
            if visible.len() >= cap {
                break;
            }
            if let Some(item) = items.iter().find(|item| item.contains(needle)) {
                visible.push(item.clone());
            }
        }
        return visible;
    }

    if let Some(primary) = items.first() {
        visible.push(primary.clone());
    }
    if visible
        .first()
        .is_some_and(|item| item.contains(" continue waiting panes"))
    {
        for needle in [
            " send lane",
            " zoom pane",
            " mute alert",
            " unmute alert",
            " remove from send list",
            " add to send list",
            " send Enter",
        ] {
            if visible.len() >= cap {
                break;
            }
            if let Some(item) = items
                .iter()
                .find(|item| item.contains(needle) && !visible.contains(item))
            {
                visible.push(item.clone());
            }
        }
    }
    if visible.len() < cap
        && let Some(zoom) = items
            .iter()
            .find(|item| item.contains(" zoom pane") && !visible.contains(item))
    {
        visible.push(zoom.clone());
    }
    for item in items {
        if visible.len() >= cap {
            break;
        }
        if !visible.contains(item) {
            visible.push(item.clone());
        }
    }

    visible
}

fn prioritize_overview_overlay_lines(lines: Vec<String>, max_rows: usize) -> Vec<String> {
    if lines.len() <= max_rows || max_rows == 0 || max_rows > 12 {
        return lines;
    }

    let mut prelude = Vec::new();
    let mut queue = Vec::new();
    let mut watching = Vec::new();
    let mut lanes = Vec::new();
    let mut queue_heading = String::from("Queue");
    let mut section = None::<&str>;

    for line in lines {
        if is_queue_heading(&line) {
            queue_heading = line;
            section = Some("Queue");
            continue;
        }

        match line.as_str() {
            "Watching" => {
                section = Some("Watching");
            }
            "Lanes" => {
                section = Some("Lanes");
            }
            _ => match section {
                Some("Queue") if !line.is_empty() => queue.push(line),
                Some("Watching") if !line.is_empty() => watching.push(line),
                Some("Lanes") if !line.is_empty() => lanes.push(line),
                _ if !line.is_empty() => prelude.push(line),
                _ => {}
            },
        }
    }

    let compact = max_rows <= 8;
    let mut prioritized = prioritize_overview_prelude_lines(prelude, 4.min(max_rows));

    if !queue.is_empty() {
        if compact {
            let available = max_rows.saturating_sub(prioritized.len());
            prioritized.extend(prioritize_queue_lines(&queue, available.min(3)));
        } else {
            let separator = usize::from(!prioritized.is_empty());
            if prioritized.len().saturating_add(separator + 2) <= max_rows {
                if !prioritized.is_empty() {
                    prioritized.push(String::new());
                }
                prioritized.push(queue_heading);
                let available = max_rows.saturating_sub(prioritized.len());
                prioritized.extend(prioritize_queue_lines(&queue, available.min(2)));
            }
        }
    }

    if !watching.is_empty() {
        let separator = usize::from(!compact && !prioritized.is_empty());
        if prioritized.len().saturating_add(separator + 2) <= max_rows {
            if separator == 1 {
                prioritized.push(String::new());
            }
            prioritized.push(String::from("Watching"));
            let available = max_rows.saturating_sub(prioritized.len());
            prioritized.extend(watching.into_iter().take(available.min(2)));
        }
    }

    if !lanes.is_empty() {
        let separator = usize::from(!compact && !prioritized.is_empty());
        if prioritized.len().saturating_add(separator + 2) <= max_rows {
            if separator == 1 {
                prioritized.push(String::new());
            }
            prioritized.push(String::from("Lanes"));
            let available = max_rows.saturating_sub(prioritized.len());
            let mut visible = lanes.iter().take(available).cloned().collect::<Vec<_>>();
            if !visible.iter().any(|line| line.starts_with('>'))
                && let Some(selected_lane) = lanes.iter().find(|line| line.starts_with('>'))
                && let Some(last) = visible.last_mut()
            {
                *last = selected_lane.clone();
            }
            prioritized.extend(visible);
        }
    }

    if prioritized.is_empty() {
        Vec::new()
    } else {
        prioritized
    }
}

fn prioritize_queue_lines(queue: &[String], limit: usize) -> Vec<String> {
    if limit == 0 || queue.is_empty() {
        return Vec::new();
    }

    let count = limit.min(queue.len());
    let overflow = queue.last().filter(|line| is_attention_overflow_line(line));
    if queue.len() > count
        && count >= 2
        && let Some(overflow) = overflow
    {
        let mut visible = queue.iter().take(count - 1).cloned().collect::<Vec<_>>();
        visible.push(overflow.clone());
        return visible;
    }

    queue.iter().take(count).cloned().collect()
}

fn is_attention_overflow_line(line: &str) -> bool {
    line.starts_with("+ ")
        && (line.contains(" more need you: ") || line.contains(" more needs you: "))
}

fn prioritize_overview_prelude_lines(lines: Vec<String>, cap: usize) -> Vec<String> {
    let mut prioritized = Vec::new();

    for prefix in [
        "All clear:",
        "Action:",
        "Needs you:",
        "Target:",
        "Start:",
        "Working:",
    ] {
        if prioritized.len() >= cap {
            break;
        }
        if let Some(line) = lines.iter().find(|line| line.starts_with(prefix))
            && !prioritized.iter().any(|entry| entry == line)
        {
            prioritized.push(line.clone());
        }
    }

    for line in lines {
        if prioritized.len() >= cap {
            break;
        }
        if !prioritized.iter().any(|entry| entry == &line) {
            prioritized.push(line);
        }
    }

    prioritized
}

#[cfg(test)]
fn prioritize_output_overlay_lines(lines: Vec<String>, max_rows: usize) -> Vec<String> {
    prioritize_output_overlay_lines_with_scroll(lines, max_rows, 0).lines
}

fn prioritize_output_overlay_lines_with_scroll(
    lines: Vec<String>,
    max_rows: usize,
    offset_from_bottom: usize,
) -> PreparedPanelLines {
    if lines.len() <= max_rows || max_rows == 0 {
        return match section_scroll_metrics(&lines, "Latest", &["Summary", "Latest"]) {
            Some(metrics) => PreparedPanelLines::with_scroll_metrics(lines, metrics),
            None => PreparedPanelLines::without_scroll(lines),
        };
    }

    let mut prelude = Vec::new();
    let mut summary = Vec::new();
    let mut latest = Vec::new();
    let mut section = None::<&str>;

    for line in lines {
        match line.as_str() {
            "Summary" => section = Some("Summary"),
            "Latest" => section = Some("Latest"),
            _ => match section {
                Some("Summary") if !line.is_empty() => summary.push(line),
                Some("Latest") if !line.is_empty() => latest.push(line),
                _ if !line.is_empty() => prelude.push(line),
                _ => {}
            },
        }
    }

    let compact = max_rows <= 10;
    let mut prioritized = prelude.into_iter().take(2).collect::<Vec<_>>();
    let has_summary = !summary.is_empty();
    let include_summary = has_summary
        && !summary_would_crowd_scrollable_latest(&prioritized, &latest, max_rows, compact);
    let latest_for_scroll = if include_summary {
        latest.clone()
    } else {
        let mut combined = latest.clone();
        for line in &summary {
            if !combined.iter().any(|entry| entry == line) {
                combined.push(line.clone());
            }
        }
        combined
    };
    let mut showed_output_section = false;

    if include_summary {
        let separator = usize::from(!compact && !prioritized.is_empty());
        if prioritized.len().saturating_add(separator + 2) <= max_rows {
            if separator == 1 {
                prioritized.push(String::new());
            }
            prioritized.push(String::from("Summary"));
            let available = max_rows.saturating_sub(prioritized.len());
            prioritized.extend(summary.iter().take(available.min(1)).cloned());
            showed_output_section = true;
        }
    }

    if !latest_for_scroll.is_empty() {
        let separator = usize::from(!compact && !prioritized.is_empty());
        if prioritized.len().saturating_add(separator + 2) <= max_rows {
            if separator == 1 {
                prioritized.push(String::new());
            }
            prioritized.push(String::from("Latest"));
            let available = max_rows.saturating_sub(prioritized.len());
            let (visible, viewport, scroll) = prioritize_output_lines_for_scroll(
                latest_for_scroll.clone(),
                available,
                offset_from_bottom,
            );
            let track_start = prioritized.len();
            let track_len = visible.len();
            prioritized.extend(visible);
            showed_output_section = true;
            let metrics = viewport.metrics(latest_for_scroll.len());
            if metrics.viewport_len > 0 {
                return PreparedPanelLines {
                    lines: prioritized,
                    scroll_metrics: Some(metrics),
                    scroll: scroll.map(|scroll| scroll.at_track(track_start, track_len)),
                };
            }
        }
    }

    if !showed_output_section && max_rows > 0 {
        let identity_slots = max_rows.saturating_sub(1).min(1);
        let mut fallback = prioritized
            .into_iter()
            .take(identity_slots)
            .collect::<Vec<_>>();
        let remaining = max_rows.saturating_sub(fallback.len());
        let track_start = fallback.len();
        let latest_total = latest_for_scroll.len();
        let (latest_lines, viewport, scroll) =
            prioritize_output_lines_for_scroll(latest_for_scroll, remaining, offset_from_bottom);
        let latest_count = latest_lines.len();
        for line in latest_lines {
            if !fallback.iter().any(|entry| entry == &line) {
                fallback.push(line);
            }
        }
        if latest_count < remaining {
            for line in summary.into_iter().take(remaining - latest_count) {
                if !fallback.iter().any(|entry| entry == &line) {
                    fallback.push(line);
                }
            }
        }
        return PreparedPanelLines {
            lines: fallback,
            scroll_metrics: (viewport.height > 0).then_some(viewport.metrics(latest_total)),
            scroll: scroll.map(|scroll| scroll.at_track(track_start, latest_count)),
        };
    }

    PreparedPanelLines::without_scroll(prioritized)
}

fn summary_would_crowd_scrollable_latest(
    prioritized: &[String],
    latest: &[String],
    max_rows: usize,
    compact: bool,
) -> bool {
    if latest.is_empty() {
        return false;
    }

    let summary_separator = usize::from(!compact && !prioritized.is_empty());
    let latest_separator_with_summary =
        usize::from(!compact && prioritized.len().saturating_add(summary_separator + 2) > 0);
    let latest_slots_with_summary = max_rows.saturating_sub(
        prioritized.len() + summary_separator + 2 + latest_separator_with_summary + 1,
    );
    let latest_separator_without_summary = usize::from(!compact && !prioritized.is_empty());
    let latest_slots_without_summary =
        max_rows.saturating_sub(prioritized.len() + latest_separator_without_summary + 1);

    latest.len() > latest_slots_with_summary
        && latest_slots_without_summary > latest_slots_with_summary
}

fn prioritize_send_overlay_lines(lines: Vec<String>, max_rows: usize) -> Vec<String> {
    if lines.len() <= max_rows || max_rows == 0 || max_rows > 12 {
        return lines;
    }

    let mut send_to = None::<String>;
    let mut send_list = None::<String>;
    let mut hidden_panes = None::<String>;
    let mut fleet = None::<String>;
    let mut suggested = None::<String>;
    let mut start = None::<String>;
    let mut vars = None::<String>;
    let mut send_text = None::<String>;
    let mut alerts = Vec::new();
    let mut fallback = Vec::new();
    let mut confirm = Vec::new();
    let mut preview = Vec::new();
    let mut reports = Vec::new();
    let mut section = None::<&str>;

    for line in lines {
        match line.as_str() {
            "alerts" => section = Some("alerts"),
            "review" => section = Some("review"),
            "targets" | "Targets" => section = Some("targets"),
            "preview" | "Preview" => section = Some("preview"),
            "reports" | "Reports" => section = Some("reports"),
            "fleets" | "Fleets" | "groups" | "Groups" | "recent" | "Recent" | "macros"
            | "Macros" => section = Some("skip"),
            _ => match section {
                _ if line.starts_with("vars ") => vars = Some(line),
                Some("alerts") if !line.is_empty() => alerts.push(line),
                Some("review" | "targets") if !line.is_empty() => confirm.push(line),
                Some("preview") if !line.is_empty() => preview.push(line),
                Some("reports") if !line.is_empty() => reports.push(line),
                Some("skip") => {}
                _ if !line.is_empty() => {
                    if line.starts_with("send to ")
                        || line.starts_with("To: ")
                        || line.starts_with("Reply to: ")
                    {
                        send_to = Some(line);
                    } else if line.starts_with("send list ") {
                        let should_replace = send_list.as_ref().is_none_or(|current| {
                            !current.contains("hidden") && line.contains("hidden")
                        });
                        if should_replace {
                            send_list = Some(line);
                        }
                    } else if line.starts_with("Text: ") {
                        send_text = Some(line);
                    } else if line.contains("pane hidden by current view")
                        || line.contains("panes hidden by current view")
                    {
                        hidden_panes = Some(line);
                    } else if line.starts_with("fleet ") {
                        fleet = Some(line);
                    } else if line.starts_with("Action: ") {
                        suggested = Some(line);
                    } else if line.starts_with("start ") {
                        start = Some(line);
                    } else {
                        fallback.push(line);
                    }
                }
                _ => {}
            },
        }
    }

    let mut prioritized = Vec::new();
    let top_line_cap = max_rows.min(4);
    let review_style_send = send_to
        .as_deref()
        .is_some_and(|line| line.starts_with("To: "));
    let top_candidates = if review_style_send {
        vec![
            send_to,
            hidden_panes,
            send_text,
            send_list,
            fleet,
            suggested,
            start,
        ]
    } else {
        vec![
            send_to,
            send_list,
            hidden_panes,
            send_text,
            fleet,
            suggested,
            start,
        ]
    };
    for line in top_candidates.into_iter().flatten() {
        if prioritized.len() < top_line_cap {
            prioritized.push(line);
        }
    }

    if prioritized.len() < top_line_cap {
        for line in alerts {
            if prioritized.len() >= top_line_cap {
                break;
            }
            prioritized.push(line);
        }
    }

    if prioritized.is_empty() {
        prioritized.extend(fallback.into_iter().take(top_line_cap));
    }

    let has_confirm = !confirm.is_empty();
    let has_preview = !preview.is_empty();

    for (heading, items, cap) in [
        ("Targets", confirm, 4_usize),
        ("Preview", preview, 3_usize),
        ("reports", reports, 2_usize),
    ] {
        if items.is_empty() {
            continue;
        }
        let separator = usize::from(!prioritized.is_empty() && heading != "Targets");
        if prioritized.len().saturating_add(separator + 2) > max_rows {
            continue;
        }
        if !prioritized.is_empty() && heading != "Targets" {
            prioritized.push(String::new());
        }
        prioritized.push(String::from(heading));
        let available = max_rows.saturating_sub(prioritized.len());
        prioritized.extend(prioritize_send_section_items(
            heading,
            items,
            cap.min(available),
        ));
    }

    if prioritized.len() < max_rows
        && !has_preview
        && !has_confirm
        && let Some(line) = vars
    {
        let separator = usize::from(!prioritized.is_empty());
        if prioritized.len().saturating_add(separator + 1) <= max_rows {
            if !prioritized.is_empty() {
                prioritized.push(String::new());
            }
            prioritized.push(line);
        }
    }

    if prioritized.is_empty() {
        Vec::new()
    } else {
        prioritized
    }
}

fn prioritize_send_section_items(heading: &str, items: Vec<String>, cap: usize) -> Vec<String> {
    if cap == 0 {
        return Vec::new();
    }

    match heading {
        "review" => prioritize_confirm_send_items(items, cap),
        "targets" | "Targets" => prioritize_confirm_send_items(items, cap),
        "preview" | "Preview" => prioritize_preview_send_items(items, cap),
        _ => prioritize_generic_send_items(items, cap),
    }
}

fn prioritize_selected_section_items(
    heading: &str,
    items: Vec<String>,
    cap: usize,
    offset_from_bottom: usize,
) -> (Vec<String>, Option<ScrollMetrics>, Option<ScrollIndicator>) {
    if cap == 0 {
        return (Vec::new(), None, None);
    }

    match heading {
        "Output" => {
            let total = items.len();
            let (visible, viewport, scroll) =
                prioritize_output_lines_for_scroll(items, cap, offset_from_bottom);
            (visible, Some(viewport.metrics(total)), scroll)
        }
        "Agent report" => {
            let mut visible = Vec::new();
            for prefix in ["Status:", "Problem:", "Blocked:", "Action:", "Seen:"] {
                if visible.len() >= cap {
                    break;
                }
                if let Some(line) = items.iter().find(|line| line.starts_with(prefix))
                    && !visible.iter().any(|entry| entry == line)
                {
                    visible.push(line.clone());
                }
            }
            (visible, None, None)
        }
        _ => (items.into_iter().take(cap).collect(), None, None),
    }
}

#[cfg(test)]
fn prioritize_output_latest_lines(items: Vec<String>, cap: usize) -> Vec<String> {
    prioritize_output_lines_for_scroll(items, cap, 0).0
}

fn prioritize_output_lines_for_scroll(
    items: Vec<String>,
    cap: usize,
    offset_from_bottom: usize,
) -> (Vec<String>, ScrollViewport, Option<ScrollIndicator>) {
    if cap == 0 {
        let viewport = ScrollViewport::new(items.len(), 0, offset_from_bottom);
        return (Vec::new(), viewport, None);
    }

    let viewport = ScrollViewport::new(items.len(), cap, offset_from_bottom);
    let visible = items[viewport.start..viewport.end].to_vec();
    let scroll = viewport.indicator(items.len());
    (visible, viewport, scroll)
}

fn prioritize_confirm_send_items(items: Vec<String>, cap: usize) -> Vec<String> {
    let summary = items
        .iter()
        .find(|line| {
            line.starts_with("To: ")
                || line.starts_with("send to ")
                || line.starts_with("send list ")
        })
        .cloned();
    let hidden = items
        .iter()
        .find(|line| {
            line.contains("pane hidden by current view")
                || line.contains("panes hidden by current view")
        })
        .cloned();
    let send = items
        .iter()
        .find(|line| {
            line.starts_with("Text: ")
                || (line.starts_with("send ")
                    && !line.starts_with("send to ")
                    && !line.starts_with("send list "))
        })
        .cloned();
    let hidden_example = items
        .iter()
        .find(|line| line.starts_with("  ") && line.contains("(hidden)") && !line.contains("..."))
        .cloned();
    let example = hidden_example.or_else(|| {
        items
            .iter()
            .find(|line| line.starts_with("  ") && !line.contains("..."))
            .cloned()
    });
    let overflow = items.iter().find(|line| line.contains("...")).cloned();

    let mut visible = Vec::new();
    for line in [summary, hidden, send, example].into_iter().flatten() {
        if visible.len() < cap && !visible.iter().any(|entry| entry == &line) {
            visible.push(line);
        }
    }

    if let Some(overflow_line) = overflow
        && visible.len() < cap
    {
        visible.push(overflow_line);
    }

    if visible.len() < cap {
        for line in items {
            if visible.len() >= cap {
                break;
            }
            if !visible.iter().any(|entry| entry == &line) {
                visible.push(line);
            }
        }
    }

    visible
}

fn prioritize_preview_send_items(items: Vec<String>, cap: usize) -> Vec<String> {
    let overflow = items.iter().find(|line| line.contains("...")).cloned();
    let mut candidates = items
        .iter()
        .filter(|line| !line.contains("..."))
        .cloned()
        .collect::<Vec<_>>();
    if let Some(hidden_index) = candidates.iter().position(|line| line.contains("(hidden)")) {
        let hidden = candidates.remove(hidden_index);
        candidates.insert(0, hidden);
    }
    let mut visible = candidates
        .into_iter()
        .take(cap.saturating_sub(usize::from(overflow.is_some() && cap > 1)))
        .collect::<Vec<_>>();

    if let Some(overflow_line) = overflow
        && visible.len() < cap
    {
        visible.push(overflow_line);
    }

    if visible.is_empty() {
        prioritize_generic_send_items(items, cap)
    } else {
        visible
    }
}

fn prioritize_generic_send_items(items: Vec<String>, cap: usize) -> Vec<String> {
    let overflow = items.iter().find(|line| line.contains("...")).cloned();
    let mut visible = items
        .into_iter()
        .filter(|line| !line.contains("..."))
        .take(cap)
        .collect::<Vec<_>>();

    if let Some(overflow_line) = overflow
        && visible.len() == cap
    {
        visible.pop();
        visible.push(overflow_line);
    }

    visible
}

fn truncate_panel_line(line: &str, width: u16) -> String {
    let max_chars = usize::from(width);
    if max_chars == 0 {
        return String::new();
    }

    let mut chars = line.chars();
    let truncated = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() && max_chars > 3 {
        let mut shortened = truncated.chars().take(max_chars - 3).collect::<String>();
        shortened.push_str("...");
        shortened
    } else {
        truncated
    }
}

fn style_panel_lines(theme: Theme, lines: Vec<String>) -> Vec<Line<'static>> {
    lines
        .into_iter()
        .enumerate()
        .map(|(index, line)| style_panel_line(theme, index, line))
        .collect()
}

fn style_panel_line(theme: Theme, index: usize, line: String) -> Line<'static> {
    if line.is_empty() {
        return Line::default();
    }

    if is_section_heading(&line) {
        return Line::styled(line, theme.section_style());
    }

    if let Some(styled) = style_state_and_tool_line(theme, &line) {
        return styled;
    }

    if let Some(styled) = style_report_line(theme, &line) {
        return styled;
    }

    if let Some((label, value)) = split_label_value(&line) {
        let style = style_labeled_value(theme, &label, &value);
        return Line::from(vec![
            Span::styled(label, theme.section_style()),
            Span::styled(value, style),
        ]);
    }

    if line.starts_with("  ") {
        return Line::styled(line, theme.body_style());
    }

    if line.starts_with('>') {
        return Line::styled(line, theme.accent_style().add_modifier(Modifier::BOLD));
    }

    if index == 0 {
        return Line::styled(line, theme.body_style().add_modifier(Modifier::BOLD));
    }

    Line::styled(line, theme.muted_style())
}

fn style_state_and_tool_line(theme: Theme, line: &str) -> Option<Line<'static>> {
    let state = line.strip_prefix("State: ")?;
    let (status, tool) = state.split_once("   Tool: ")?;

    Some(Line::from(vec![
        Span::styled("State: ", theme.section_style()),
        Span::styled(status.to_owned(), state_value_style(theme, status)),
        Span::raw("   "),
        Span::styled("Tool: ", theme.section_style()),
        Span::styled(tool.to_owned(), theme.body_style()),
    ]))
}

fn style_report_line(theme: Theme, line: &str) -> Option<Line<'static>> {
    for label in ["Status: ", "Seen: "] {
        if let Some(value) = line.strip_prefix(label) {
            return Some(Line::from(vec![
                Span::styled(label.to_owned(), theme.section_style()),
                Span::styled(value.to_owned(), style_report_value(theme, label, value)),
            ]));
        }
    }

    None
}

fn style_labeled_value(theme: Theme, label: &str, value: &str) -> Style {
    match label {
        "Status: " => state_value_style(theme, value),
        "Lifecycle: " => theme.accent_style(),
        "Mission: " => theme.body_style(),
        "Blocked: " | "Problem: " => theme.warning_style(),
        "Action: " => theme.accent_style(),
        "All clear: " => theme.success_style(),
        "Now: " => theme.accent_style(),
        "Needs you: " => theme.warning_style(),
        "Working: " => theme.accent_style(),
        "Target: " => theme.body_style(),
        "Send: " => theme.success_style(),
        "Start: " => theme.body_style(),
        "Find: " => theme.body_style(),
        "Move: " => theme.body_style(),
        "Views: " => theme.body_style(),
        "More: " => theme.body_style(),
        "To: " => theme.accent_style(),
        "Reply to: " => theme.accent_style(),
        "Text: " => theme.body_style(),
        "Legend: " => theme.muted_style(),
        "Close: " => theme.muted_style(),
        "In: " => theme.accent_style(),
        "Folder: " => theme.body_style(),
        "Window: " => theme.body_style(),
        "Command: " => theme.body_style(),
        "Presets: " => theme.muted_style(),
        "Error: " => theme.danger_style(),
        "Queue: " => theme.warning_style(),
        "Updated: " => theme.body_style(),
        "Review: " => theme.warning_style(),
        "Lane: " => theme.muted_style(),
        "View: " => theme.muted_style(),
        "Seen: " => theme.muted_style(),
        _ => theme.body_style(),
    }
}

fn style_report_value(theme: Theme, label: &str, value: &str) -> Style {
    match label {
        "Status: " => state_value_style(theme, value),
        "Seen: " => theme.muted_style(),
        _ => theme.body_style(),
    }
}

fn state_value_style(theme: Theme, value: &str) -> Style {
    match value
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "waiting" => theme.warning_style(),
        "error" | "stuck" => theme.danger_style(),
        "running" => theme.accent_style(),
        "done" => theme.success_style(),
        "idle" | "checking" | "unknown" => theme.muted_style(),
        _ => theme.body_style(),
    }
}

fn split_label_value(line: &str) -> Option<(String, String)> {
    for label in [
        "State: ",
        "Status: ",
        "Lifecycle: ",
        "Mission: ",
        "Problem: ",
        "Blocked: ",
        "Action: ",
        "All clear: ",
        "Now: ",
        "Needs you: ",
        "Working: ",
        "Target: ",
        "Send: ",
        "Start: ",
        "Find: ",
        "Move: ",
        "Views: ",
        "More: ",
        "To: ",
        "Reply to: ",
        "Text: ",
        "Legend: ",
        "Close: ",
        "In: ",
        "Folder: ",
        "Window: ",
        "Command: ",
        "Presets: ",
        "Error: ",
        "Queue: ",
        "Updated: ",
        "Lane: ",
        "View: ",
        "Review: ",
        "Seen: ",
    ] {
        if let Some(value) = line.strip_prefix(label) {
            return Some((label.to_owned(), value.to_owned()));
        }
    }
    None
}

fn is_section_heading(line: &str) -> bool {
    if is_queue_heading(line) {
        return true;
    }

    matches!(
        line,
        "Command"
            | "Agent report"
            | "Output"
            | "Summary"
            | "start"
            | "Start"
            | "view"
            | "View"
            | "send list"
            | "Send List"
            | "pane"
            | "Pane"
            | "settings"
            | "Settings"
            | "reports"
            | "Reports"
            | "alerts"
            | "Alerts"
            | "fleets"
            | "Fleets"
            | "groups"
            | "Groups"
            | "recent"
            | "Recent"
            | "review"
            | "Review"
            | "targets"
            | "Targets"
            | "macros"
            | "Macros"
            | "preview"
            | "Preview"
            | "queue"
            | "Queue"
            | "watching"
            | "Watching"
            | "lanes"
            | "Lanes"
    )
}

fn is_queue_heading(line: &str) -> bool {
    line == "Queue"
        || line
            .strip_prefix("Queue (")
            .and_then(|tail| tail.strip_suffix(')'))
            .is_some_and(|count| !count.is_empty() && count.chars().all(|ch| ch.is_ascii_digit()))
}

#[cfg(test)]
fn top_centered_rect(area: Rect, width: u16, height: u16, top_margin: u16) -> Rect {
    top_centered_rect_with_min(area, width, height, top_margin, 6)
}

fn top_centered_rect_with_min(
    area: Rect,
    width: u16,
    height: u16,
    top_margin: u16,
    min_height: u16,
) -> Rect {
    let popup_width = width.min(area.width.saturating_sub(2)).max(24);
    let popup_height = height.min(area.height.saturating_sub(2)).max(min_height);
    let margin = top_margin.min(area.height.saturating_sub(popup_height));
    let top = area.y.saturating_add(margin);
    let left = area
        .x
        .saturating_add(area.width.saturating_sub(popup_width) / 2);
    Rect::new(left, top, popup_width, popup_height)
}

fn overlay_rect(body: Rect, title: &str, lines: &[String]) -> Rect {
    let longest_line = lines
        .iter()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(title.chars().count());
    let kind = overlay_kind(title);
    let max_width = body.width.saturating_sub(2).max(24);
    let max_height = body.height.saturating_sub(1).max(8);
    let target_width = match kind {
        OverlayKind::Output => body.width.saturating_mul(2) / 3,
        OverlayKind::Actions => body.width.saturating_mul(3) / 5,
        OverlayKind::Help => 74,
        OverlayKind::Browse => body.width.saturating_mul(11) / 20,
        OverlayKind::CommandCenter => body.width.saturating_mul(11) / 20,
        OverlayKind::Generic => body.width.saturating_mul(11) / 20,
    };
    let min_width = match kind {
        OverlayKind::Output => 60,
        OverlayKind::Actions => 52,
        OverlayKind::Help => 68,
        OverlayKind::Browse => 44,
        OverlayKind::CommandCenter => 44,
        OverlayKind::Generic => 44,
    };
    let width = (longest_line as u16)
        .saturating_add(6)
        .max(target_width.min(max_width))
        .clamp(min_width.min(max_width), max_width);
    let min_height = match kind {
        OverlayKind::Output if lines.iter().any(|line| line == "Latest") => 10,
        OverlayKind::Output => 6,
        OverlayKind::Actions => 12,
        OverlayKind::Help => 8,
        OverlayKind::Browse if lines.len() <= 2 => 4,
        OverlayKind::Browse => 8,
        OverlayKind::CommandCenter if lines.len() <= 2 => 4,
        OverlayKind::CommandCenter => 8,
        OverlayKind::Generic => 8,
    };
    let height = (lines.len() as u16)
        .saturating_add(2)
        .clamp(min_height.min(max_height), max_height);
    let rect_min_height = match kind {
        OverlayKind::Browse if lines.len() <= 2 => 4,
        OverlayKind::CommandCenter if lines.len() <= 2 => 4,
        _ => 6,
    };
    top_centered_rect_with_min(body, width, height, 1, rect_min_height)
}

fn overlay_kind(title: &str) -> OverlayKind {
    match title {
        "Output" => OverlayKind::Output,
        "More" => OverlayKind::Actions,
        "Help" => OverlayKind::Help,
        "Browse" => OverlayKind::Browse,
        "Command Center" => OverlayKind::CommandCenter,
        _ => OverlayKind::Generic,
    }
}

fn matches_any(
    bindings: &crate::app::KeyBindingsConfig,
    code: &KeyCode,
    accessor: impl Fn(&crate::app::KeyBindingsConfig) -> &[String],
) -> bool {
    key_token(code)
        .as_deref()
        .is_some_and(|token| accessor(bindings).iter().any(|binding| binding == token))
}

fn key_token(code: &KeyCode) -> Option<String> {
    match code {
        KeyCode::Char(' ') => Some(String::from("space")),
        KeyCode::Char(ch) => Some(ch.to_string()),
        KeyCode::Enter => Some(String::from("enter")),
        KeyCode::Tab => Some(String::from("tab")),
        KeyCode::Backspace => Some(String::from("backspace")),
        KeyCode::Esc => Some(String::from("esc")),
        KeyCode::Up => Some(String::from("up")),
        KeyCode::Down => Some(String::from("down")),
        _ => None,
    }
}

fn init_terminal() -> Result<TuiTerminal> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    Ok(Terminal::new(backend)?)
}

fn restore_terminal(terminal: &mut TuiTerminal) -> Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

fn ring_terminal_bell() -> Result<()> {
    let mut stdout = io::stdout();
    stdout.write_all(b"\x07")?;
    stdout.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        App, BOARD_LOCATION_MAX_WIDTH, BOARD_LOCATION_MIN_WIDTH, BoardLayoutMode, BodyLayoutMode,
        INPUT_POLL_TIMEOUT, KeyPattern, LayoutProfile, PeekToggleKeys, QUEUED_INPUT_DRAIN_LIMIT,
        ResolvedBodyLayout, ScrollbarGeometry, TerminalProfile, Theme, append_cell_ellipsis,
        board_cells, board_constraints, board_headers, board_location_width,
        board_rows_for_capacity, draw, draw_with_profile, handle_key_event, handle_key_press,
        is_section_heading, key_token, locale_is_utf8, normalized_app_key_code, overlay_rect,
        prepare_context_panel_lines, prepare_overlay_lines, prioritize_action_section_items,
        prioritize_actions_overlay_lines, prioritize_confirm_send_items,
        prioritize_context_panel_lines, prioritize_fleet_picker_lines,
        prioritize_generic_send_items, prioritize_output_latest_lines,
        prioritize_output_overlay_lines, prioritize_overview_overlay_lines,
        prioritize_preview_send_items, prioritize_selected_section_items,
        prioritize_send_overlay_lines, prioritize_send_section_items, selected_board_latest_lines,
        split_label_value, top_centered_rect, truncate_cell, truncate_location_cell,
        truncate_panel_line, wrap_panel_line, wrap_words,
    };
    use crate::app::{
        BoardRow, LayoutPreset, ThemeColor, ThemeConfig, ThemeOverrides, ThemePreset, UiSettings,
        tests::{
            PanelFixture, ViewModelFixture, app_from_panel_fixture, app_from_view_model_fixture,
            app_with_panes, apply_rebound_keybindings, fake_tmux_script, load_panel_fixtures,
            load_view_model_fixtures, mark_pane_done_for_review, mark_pane_running_agent,
            mark_pane_runtime_stale, remember_command_for_test, sample_pane,
            set_live_partial_runtime, set_pane_report_fields, set_runtime_lines_without_age,
            use_fake_tmux_for_test, use_notification_mode_for_test,
        },
    };
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::style::{Color, Modifier};
    use ratatui::{
        Terminal,
        backend::TestBackend,
        buffer::Buffer,
        layout::{Constraint, Layout, Rect},
    };
    use std::{
        collections::HashMap,
        time::{Duration, Instant},
    };

    const ALL_THEME_PRESETS: [ThemePreset; 11] = [
        ThemePreset::Calm,
        ThemePreset::Contrast,
        ThemePreset::Mono,
        ThemePreset::TerminalNative,
        ThemePreset::CatppuccinLatte,
        ThemePreset::CatppuccinMocha,
        ThemePreset::TokyoNight,
        ThemePreset::GruvboxDark,
        ThemePreset::GruvboxLight,
        ThemePreset::Nord,
        ThemePreset::RosePine,
    ];

    fn rgb_tuple(color: Color) -> Option<(u8, u8, u8)> {
        match color {
            Color::Rgb(red, green, blue) => Some((red, green, blue)),
            _ => None,
        }
    }

    fn rgb_hex(hex: u32) -> (u8, u8, u8) {
        (
            ((hex >> 16) & 0xFF) as u8,
            ((hex >> 8) & 0xFF) as u8,
            (hex & 0xFF) as u8,
        )
    }

    fn contrast_ratio(first: (u8, u8, u8), second: (u8, u8, u8)) -> f64 {
        let first_luminance = relative_luminance(first);
        let second_luminance = relative_luminance(second);
        let lighter = first_luminance.max(second_luminance);
        let darker = first_luminance.min(second_luminance);
        (lighter + 0.05) / (darker + 0.05)
    }

    fn relative_luminance((red, green, blue): (u8, u8, u8)) -> f64 {
        fn channel(value: u8) -> f64 {
            let normalized = f64::from(value) / 255.0;
            if normalized <= 0.03928 {
                normalized / 12.92
            } else {
                ((normalized + 0.055) / 1.055).powf(2.4)
            }
        }

        0.2126 * channel(red) + 0.7152 * channel(green) + 0.0722 * channel(blue)
    }

    fn render_lines(app: &App, width: u16, height: u16) -> Vec<String> {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("test terminal should build");
        terminal
            .draw(|frame| draw(frame, app))
            .expect("test draw should succeed");
        buffer_lines(terminal.backend().buffer())
    }

    fn render_buffer(app: &App, width: u16, height: u16) -> Buffer {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("test terminal should build");
        terminal
            .draw(|frame| draw(frame, app))
            .expect("test draw should succeed");
        terminal.backend().buffer().clone()
    }

    fn render_buffer_with_profile(
        app: &App,
        width: u16,
        height: u16,
        profile: TerminalProfile,
    ) -> Buffer {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("test terminal should build");
        terminal
            .draw(|frame| draw_with_profile(frame, app, profile))
            .expect("test draw should succeed");
        terminal.backend().buffer().clone()
    }

    fn render_grid(app: &App, width: u16, height: u16) -> Vec<String> {
        let buffer = render_buffer(app, width, height);
        buffer_grid_lines(&buffer)
    }

    fn render_grid_on_terminal(terminal: &mut Terminal<TestBackend>, app: &App) -> Vec<String> {
        draw_on_terminal(terminal, app);
        buffer_grid_lines(terminal.backend().buffer())
    }

    fn draw_on_terminal(terminal: &mut Terminal<TestBackend>, app: &App) {
        terminal
            .draw(|frame| draw(frame, app))
            .expect("test draw should succeed");
    }

    fn board_row(location: &str, status: &str, latest: &str) -> BoardRow {
        BoardRow {
            selected: false,
            active: false,
            marked: false,
            targeted: false,
            staged: false,
            show_command_in_latest: false,
            attention: String::new(),
            status: status.to_owned(),
            lifecycle: status.to_owned(),
            mission: String::new(),
            heat: String::new(),
            age: String::new(),
            cpu: String::new(),
            mem: String::new(),
            lane: String::new(),
            pane: String::from("%1"),
            location: location.to_owned(),
            command: String::new(),
            title: latest.to_owned(),
        }
    }

    fn buffer_lines(buffer: &Buffer) -> Vec<String> {
        let width = buffer.area.width as usize;
        buffer
            .content
            .chunks(width)
            .map(|cells| {
                cells
                    .iter()
                    .map(|cell| cell.symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect()
    }

    fn buffer_grid_lines(buffer: &Buffer) -> Vec<String> {
        let width = buffer.area.width as usize;
        buffer
            .content
            .chunks(width)
            .map(|cells| cells.iter().map(|cell| cell.symbol()).collect::<String>())
            .collect()
    }

    fn panel_fixture(name: &str) -> PanelFixture {
        load_panel_fixtures()
            .into_iter()
            .find(|fixture| fixture.name == name)
            .unwrap_or_else(|| panic!("missing panel fixture: {name}"))
    }

    fn view_fixture(name: &str) -> ViewModelFixture {
        load_view_model_fixtures()
            .into_iter()
            .find(|fixture| fixture.name == name)
            .unwrap_or_else(|| panic!("missing view fixture: {name}"))
    }

    fn line_index(lines: &[String], needle: &str) -> usize {
        lines
            .iter()
            .position(|line| line.contains(needle))
            .unwrap_or_else(|| panic!("missing `{needle}` in screen:\n{}", lines.join("\n")))
    }

    fn text_position(lines: &[String], needle: &str) -> (usize, usize) {
        lines
            .iter()
            .enumerate()
            .find_map(|(row, line)| line.find(needle).map(|column| (column, row)))
            .unwrap_or_else(|| panic!("missing `{needle}` in screen:\n{}", lines.join("\n")))
    }

    fn panel_border_signature(lines: &[String]) -> Vec<(usize, usize, char)> {
        lines
            .iter()
            .enumerate()
            .flat_map(|(row, line)| {
                line.chars().enumerate().filter_map(move |(column, ch)| {
                    matches!(ch, '┌' | '┐' | '└' | '┘' | '─' | '│').then_some((column, row, ch))
                })
            })
            .collect()
    }

    fn line_indices(lines: &[String], needle: &str) -> Vec<usize> {
        let matches = lines
            .iter()
            .enumerate()
            .filter_map(|(index, line)| line.contains(needle).then_some(index))
            .collect::<Vec<_>>();
        assert!(
            !matches.is_empty(),
            "missing `{needle}` in screen:\n{}",
            lines.join("\n")
        );
        matches
    }

    fn body_rect(width: u16, height: u16) -> Rect {
        let area = Rect::new(0, 0, width, height);
        let [_, body, _] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(8),
            Constraint::Length(1),
        ])
        .areas(area);
        body
    }

    fn assert_no_garbled_text(lines: &[String]) {
        for line in lines {
            assert!(
                !line.contains('�') && !line.contains("â"),
                "screen contains mojibake: {line}"
            );
        }
    }

    fn screen_text(lines: &[String]) -> String {
        lines.join("\n")
    }

    fn coverage_instrumented() -> bool {
        std::env::var_os("LLVM_PROFILE_FILE").is_some()
            || std::env::var_os("CARGO_LLVM_COV").is_some()
    }

    fn github_actions() -> bool {
        std::env::var_os("GITHUB_ACTIONS").is_some()
    }

    fn normalize_relative_ages(lines: Vec<String>) -> Vec<String> {
        lines
            .into_iter()
            .map(|line| {
                let Some(suffix_start) = line.find("s ago") else {
                    return line;
                };
                let digits_start = line[..suffix_start]
                    .rfind(|ch: char| !ch.is_ascii_digit())
                    .map(|index| index + 1)
                    .unwrap_or(0);
                if digits_start == suffix_start {
                    return line;
                }
                format!("{}#{}", &line[..digits_start], &line[suffix_start..])
            })
            .collect()
    }

    fn app_with_numbered_output(prefix: &str, count: usize) -> App {
        let output = (1..=count)
            .map(|index| format!("{prefix} output {index:02}"))
            .collect::<Vec<_>>();
        let output_refs = output.iter().map(String::as_str).collect::<Vec<_>>();
        app_with_panes(vec![sample_pane("bash")], vec![("%1", output_refs)])
    }

    fn count_output_rows(lines: &[String], prefix: &str) -> usize {
        lines.iter().filter(|line| line.contains(prefix)).count()
    }

    fn scrollbar_thumb_center_row(lines: &[String]) -> f32 {
        let (_, thumb_rows) = scrollbar_rows(lines);
        let rows = thumb_rows
            .into_iter()
            .map(|row| row as f32)
            .collect::<Vec<_>>();
        assert!(
            !rows.is_empty(),
            "missing scrollbar thumb:\n{}",
            screen_text(lines)
        );
        rows.iter().sum::<f32>() / rows.len() as f32
    }

    fn scrollbar_rows(lines: &[String]) -> (Vec<usize>, Vec<usize>) {
        let cells = scrollbar_cells(lines);
        let mut track_rows = cells.iter().map(|(row, _, _)| *row).collect::<Vec<_>>();
        track_rows.dedup();
        let mut thumb_rows = cells
            .iter()
            .filter_map(|(row, _, ch)| (*ch == '█').then_some(*row))
            .collect::<Vec<_>>();
        thumb_rows.dedup();
        assert!(
            track_rows.is_empty() || track_rows.windows(2).all(|rows| rows[1] == rows[0] + 1),
            "scrollbar track should be one contiguous vertical run:\n{}",
            screen_text(lines)
        );
        assert!(
            thumb_rows.is_empty() || thumb_rows.windows(2).all(|rows| rows[1] == rows[0] + 1),
            "scrollbar thumb should be one contiguous vertical run:\n{}",
            screen_text(lines)
        );
        (track_rows, thumb_rows)
    }

    fn scrollbar_cells(lines: &[String]) -> Vec<(usize, usize, char)> {
        let cells = lines
            .iter()
            .enumerate()
            .flat_map(|(row, line)| {
                line.chars().enumerate().filter_map(move |(column, ch)| {
                    matches!(ch, '█' | '░').then_some((row, column, ch))
                })
            })
            .collect::<Vec<_>>();
        if let Some((_, column, _)) = cells.first().copied() {
            assert!(
                cells.iter().all(|(_, candidate, _)| *candidate == column),
                "scrollbar should occupy exactly one fixed column:\n{}",
                screen_text(lines)
            );
            for (row, column, _) in &cells {
                let next = lines[*row].chars().nth(column + 1);
                assert_eq!(
                    next,
                    Some('│'),
                    "scrollbar column should sit directly beside the panel border:\n{}",
                    screen_text(lines)
                );
            }
        }
        cells
    }

    fn assert_scrollbar_matches_output_rows(lines: &[String], prefix: &str, content_len: usize) {
        let output_rows = rendered_output_line_indices(lines, prefix);
        let (track_rows, thumb_rows) = scrollbar_rows(lines);
        assert_eq!(
            track_rows,
            output_rows,
            "scrollbar track should align only to visible output rows:\n{}",
            screen_text(lines)
        );

        let position_from_top = first_numbered_output_index(lines, prefix)
            .expect("visible numbered output should expose a top position")
            - 1;
        let expected = ScrollbarGeometry::new(
            content_len,
            output_rows.len(),
            position_from_top,
            track_rows.len(),
        )
        .expect("scrollable output should have geometry");
        assert_eq!(
            thumb_rows.len(),
            expected.thumb_len,
            "thumb height should match viewport/content ratio:\n{}",
            screen_text(lines)
        );
        assert_eq!(
            thumb_rows.first().copied().unwrap_or_default() - track_rows[0],
            expected.thumb_start,
            "thumb offset should match scroll position:\n{}",
            screen_text(lines)
        );
    }

    fn rendered_output_line_indices(lines: &[String], prefix: &str) -> Vec<usize> {
        let two_space = format!("│  {prefix}");
        let three_space = format!("│   {prefix}");
        let matches = lines
            .iter()
            .enumerate()
            .filter_map(|(index, line)| {
                (line.contains(&two_space) || line.contains(&three_space)).then_some(index)
            })
            .collect::<Vec<_>>();
        assert!(
            !matches.is_empty(),
            "missing rendered output rows for `{prefix}` in screen:\n{}",
            screen_text(lines)
        );
        matches
    }

    fn assert_scrollbar_thumb_at_top(lines: &[String]) {
        let (track_rows, thumb_rows) = scrollbar_rows(lines);
        assert!(
            !track_rows.is_empty() && !thumb_rows.is_empty(),
            "expected visible scrollbar:\n{}",
            screen_text(lines)
        );
        assert_eq!(
            thumb_rows.first(),
            track_rows.first(),
            "scrollbar thumb should touch the top of its track at oldest content:\n{}",
            screen_text(lines)
        );
    }

    fn assert_scrollbar_thumb_at_bottom(lines: &[String]) {
        let (track_rows, thumb_rows) = scrollbar_rows(lines);
        assert!(
            !track_rows.is_empty() && !thumb_rows.is_empty(),
            "expected visible scrollbar:\n{}",
            screen_text(lines)
        );
        assert_eq!(
            thumb_rows.last(),
            track_rows.last(),
            "scrollbar thumb should touch the bottom of its track at newest content:\n{}",
            screen_text(lines)
        );
    }

    fn first_numbered_output_index(lines: &[String], prefix: &str) -> Option<usize> {
        lines
            .iter()
            .filter_map(|line| numbered_suffix(line, prefix))
            .min()
    }

    fn numbered_suffix(line: &str, prefix: &str) -> Option<usize> {
        let start = line.find(prefix)?;
        line[start + prefix.len()..]
            .split_whitespace()
            .next()?
            .parse()
            .ok()
    }

    fn assert_no_scrollbar(lines: &[String]) {
        let screen = screen_text(lines);
        assert!(
            !screen.contains('█') && !screen.contains('░') && !screen.contains('#'),
            "screen should not render an inert scrollbar:\n{screen}"
        );
    }

    fn assert_screen_hides_raw_tmux_identity(label: &str, lines: &[String]) {
        let screen = screen_text(lines);
        for token in ["%1", "%2", "$0", "$1", "@0", "@1"] {
            assert!(
                !screen.contains(token),
                "{label} leaked raw tmux identity `{token}`:\n{screen}"
            );
        }
    }

    fn find_substring(lines: &[String], needle: &str) -> (u16, u16) {
        let y = lines
            .iter()
            .position(|line| line.contains(needle))
            .unwrap_or_else(|| panic!("missing `{needle}` in screen:\n{}", lines.join("\n")));
        let byte_x = lines[y]
            .find(needle)
            .unwrap_or_else(|| panic!("missing `{needle}` in screen:\n{}", lines.join("\n")));
        (lines[y][..byte_x].chars().count() as u16, y as u16)
    }

    fn buffer_cell<'a>(
        buffer: &'a Buffer,
        lines: &[String],
        needle: &str,
    ) -> &'a ratatui::buffer::Cell {
        let (x, y) = find_substring(lines, needle);
        buffer
            .cell((x, y))
            .unwrap_or_else(|| panic!("missing buffer cell at {x},{y} for `{needle}`"))
    }

    fn buffer_cell_in_line<'a>(
        buffer: &'a Buffer,
        line: &str,
        y: usize,
        needle: &str,
    ) -> &'a ratatui::buffer::Cell {
        let byte_x = line
            .find(needle)
            .unwrap_or_else(|| panic!("missing `{needle}` in line {y}: {line}"));
        let x = line[..byte_x].chars().count();
        buffer
            .cell((x as u16, y as u16))
            .unwrap_or_else(|| panic!("missing buffer cell at {x},{y} for `{needle}`"))
    }

    fn assert_render_invariants(lines: &[String], width: u16, height: u16) {
        assert_eq!(lines.len(), height as usize, "screen height mismatch");
        assert_no_garbled_text(lines);

        for (index, line) in lines.iter().enumerate() {
            assert_eq!(
                line.chars().count(),
                width as usize,
                "line {index} width mismatch"
            );
            assert!(
                !line.chars().any(|ch| ch.is_control() && ch != '\t'),
                "line {index} contains control characters: {line:?}"
            );
            if let Some(left) = line.find('┌') {
                let right = line.find('┐').unwrap_or_else(|| {
                    panic!("line {index} has top-left corner without top-right: {line}")
                });
                assert!(left < right, "line {index} has inverted top border: {line}");
            }
            if let Some(left) = line.find('└') {
                let right = line.find('┘').unwrap_or_else(|| {
                    panic!("line {index} has bottom-left corner without bottom-right: {line}")
                });
                assert!(
                    left < right,
                    "line {index} has inverted bottom border: {line}"
                );
            }
        }
    }

    fn assert_exact_grid(lines: &[String], width: u16, expected: &[&str]) {
        assert_no_garbled_text(lines);
        assert_eq!(lines.len(), expected.len(), "line count mismatch");
        for (index, (actual, expected)) in lines.iter().zip(expected.iter()).enumerate() {
            assert_eq!(
                actual.chars().count(),
                width as usize,
                "line {index} should span the full frame width"
            );
            assert_eq!(
                actual.trim_end(),
                *expected,
                "line {index} visible cells should match exactly"
            );
        }
    }

    fn assert_golden_grid(lines: &[String], width: u16, name: &str) {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("tui")
            .join("golden")
            .join(format!("{name}.txt"));
        if std::env::var_os("MUXBOARD_BLESS_GOLDEN").is_some() {
            let contents = lines
                .iter()
                .map(|line| line.trim_end())
                .collect::<Vec<_>>()
                .join("\n");
            std::fs::write(&path, format!("{contents}\n")).unwrap_or_else(|err| {
                panic!("failed to write golden grid {}: {err}", path.display())
            });
        }
        let contents = std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("failed to read golden grid {}: {err}", path.display()));
        let expected = contents.lines().collect::<Vec<_>>();
        assert_exact_grid(lines, width, &expected);
    }

    fn panel_spans(line: &str) -> Vec<(usize, usize)> {
        let mut spans = Vec::new();
        let mut start = None;

        for (index, ch) in line.chars().enumerate() {
            match ch {
                '┌' | '└' => start = Some(index),
                '┐' | '┘' => {
                    if let Some(start_index) = start.take() {
                        spans.push((start_index, index));
                    }
                }
                _ => {}
            }
        }

        spans
    }

    fn assert_terms_in_order(line: &str, terms: &[&str]) {
        let mut last = 0usize;
        for (index, term) in terms.iter().enumerate() {
            let position = line[last..]
                .find(term)
                .map(|offset| last + offset)
                .unwrap_or_else(|| panic!("missing `{term}` in line: {line}"));
            if index > 0 {
                assert!(position > last, "terms out of order in line: {line}");
            }
            last = position;
        }
    }

    #[test]
    fn terminal_profile_downgrades_dumb_or_non_utf8_terminals() {
        let empty_env = std::collections::HashMap::new();
        let default_profile = TerminalProfile::default();
        assert!(!default_profile.ascii_borders);
        assert!(default_profile.color);
        assert!(locale_is_utf8(&empty_env));

        let ssh_dumb = std::collections::HashMap::from([
            (String::from("TERM"), String::from("dumb")),
            (String::from("LANG"), String::from("C")),
            (String::from("NO_COLOR"), String::from("1")),
            (
                format!("TERM_{}", "PROGRAM"),
                String::from("Apple_Terminal"),
            ),
            (String::from("SSH_CONNECTION"), String::from("1 2 3 4")),
        ]);
        let profile = TerminalProfile::from_env_map(&ssh_dumb);

        assert!(profile.ascii_borders);
        assert!(!profile.color);
        assert!(!locale_is_utf8(&ssh_dumb));

        let utf8_color = std::collections::HashMap::from([
            (String::from("TERM"), String::from("xterm-256color")),
            (String::from("LANG"), String::from("en_US.UTF-8")),
        ]);
        let profile = TerminalProfile::from_env_map(&utf8_color);

        assert!(!profile.ascii_borders);
        assert!(profile.color);
        assert!(locale_is_utf8(&utf8_color));

        let non_utf8_color = std::collections::HashMap::from([
            (String::from("TERM"), String::from("xterm-256color")),
            (String::from("LC_CTYPE"), String::from("POSIX")),
        ]);
        let profile = TerminalProfile::from_env_map(&non_utf8_color);
        assert!(profile.ascii_borders);
        assert!(profile.color);
        assert!(!locale_is_utf8(&non_utf8_color));

        let color_disabled = std::collections::HashMap::from([
            (String::from("TERM"), String::from("xterm-256color")),
            (String::from("LANG"), String::from("en_US.UTF-8")),
            (String::from("CLICOLOR"), String::from("0")),
        ]);
        let profile = TerminalProfile::from_env_map(&color_disabled);
        assert!(!profile.ascii_borders);
        assert!(!profile.color);
        assert!(locale_is_utf8(&color_disabled));

        let lc_all_wins = std::collections::HashMap::from([
            (String::from("TERM"), String::from("xterm-256color")),
            (String::from("LANG"), String::from("en_US.UTF-8")),
            (String::from("LC_ALL"), String::from("C")),
            (String::from("CLICOLOR"), String::from(" 0 ")),
        ]);
        let profile = TerminalProfile::from_env_map(&lc_all_wins);
        assert!(profile.ascii_borders);
        assert!(!profile.color);
        assert!(!locale_is_utf8(&lc_all_wins));

        let lc_ctype_fallback = std::collections::HashMap::from([
            (String::from("TERM"), String::from("xterm-256color")),
            (String::from("LANG"), String::new()),
            (String::from("LC_CTYPE"), String::from("en_US.UTF8")),
            (String::from("CLICOLOR"), String::from("1")),
        ]);
        let profile = TerminalProfile::from_env_map(&lc_ctype_fallback);
        assert!(!profile.ascii_borders);
        assert!(profile.color);
        assert!(locale_is_utf8(&lc_ctype_fallback));
    }

    #[test]
    fn usability_ascii_terminal_profile_renders_without_box_drawing_or_colors() {
        let app = app_with_panes(
            vec![sample_pane("codex")],
            vec![(
                "%1",
                vec![
                    "STATUS=waiting | BLOCKER=approval | NEXT=Press Enter to continue",
                    "Waiting for approval. Continue?",
                ],
            )],
        );
        let profile = TerminalProfile {
            ascii_borders: true,
            color: false,
        };
        let buffer = render_buffer_with_profile(&app, 90, 18, profile);
        let lines = buffer_grid_lines(&buffer);
        let screen = screen_text(&lines);

        assert_render_invariants(&lines, 90, 18);
        assert!(screen.contains("+"));
        assert!(screen.contains("|"));
        for ch in ['┌', '┐', '└', '┘', '│', '─'] {
            assert!(
                !screen.contains(ch),
                "screen should use ASCII only:\n{screen}"
            );
        }
        let cell = buffer_cell(&buffer, &lines, "muxboard");
        assert_eq!(cell.fg, Color::Reset);
        assert_eq!(cell.bg, Color::Reset);
        assert!(screen.contains("Fleet"));
        assert!(screen.contains("Details"));
        assert!(screen.contains("? help"));
    }

    #[test]
    fn key_token_maps_space_to_space_binding_name() {
        assert_eq!(key_token(&KeyCode::Char(' ')).as_deref(), Some("space"));
    }

    #[test]
    fn input_loop_stays_below_human_lag_threshold() {
        let poll_timeout = std::hint::black_box(INPUT_POLL_TIMEOUT);
        let drain_limit = std::hint::black_box(QUEUED_INPUT_DRAIN_LIMIT);

        assert!(
            poll_timeout <= Duration::from_millis(50),
            "input poll timeout should stay below the threshold where movement feels laggy"
        );
        assert!(
            drain_limit >= 16,
            "queued movement keys should be drained in bursts instead of one slow frame at a time"
        );
    }

    #[test]
    fn navigation_key_burst_stays_in_memory_and_below_human_lag_threshold() {
        let panes = (0..96)
            .map(|index| {
                let mut pane = sample_pane(if index % 3 == 0 { "codex" } else { "bash" });
                pane.id = format!("%{}", index + 1);
                pane.window_id = format!("@{}", index / 6);
                pane.window_name = format!("agent-{:02}", index / 6);
                pane.pane_index = index % 6;
                pane.active = index == 0;
                pane
            })
            .collect::<Vec<_>>();
        let mut app = app_with_panes(panes, vec![]);
        let runtime = tokio::runtime::Runtime::new().expect("runtime should build");
        let selected_index = |app: &App| {
            app.board_rows(128)
                .into_iter()
                .position(|row| row.selected)
                .expect("one row should stay selected")
        };

        assert_eq!(selected_index(&app), 0);
        let started = Instant::now();
        let key_count = 128 + 32;
        runtime.block_on(async {
            for _ in 0..128 {
                handle_key_press(&mut app, KeyCode::Char('j'))
                    .await
                    .expect("movement key should stay local");
            }
            for _ in 0..32 {
                handle_key_press(&mut app, KeyCode::Up)
                    .await
                    .expect("arrow movement key should stay local");
            }
        });
        let elapsed = started.elapsed();

        assert_eq!(selected_index(&app), 0);
        let threshold = if coverage_instrumented() {
            Duration::from_secs(10)
        } else {
            Duration::from_millis(8 * key_count)
        };
        assert!(
            elapsed < threshold,
            "navigation key burst should stay under 8ms per key, took {elapsed:?}"
        );
    }

    #[test]
    fn renderer_navigation_perf_smoke_stays_interactive() {
        let panes = (0..120)
            .map(|index| {
                let mut pane = sample_pane(if index % 5 == 0 { "codex" } else { "bash" });
                pane.id = format!("%{}", index + 1);
                pane.session_id = format!("${}", index / 40);
                pane.session_name = format!("s{:02}", index / 40);
                pane.window_id = format!("@{}", index / 8);
                pane.window_name = format!("job-{:02}", index / 8);
                pane.pane_index = index % 8;
                pane.active = index == 0;
                pane
            })
            .collect::<Vec<_>>();
        let pane_ids = panes.iter().map(|pane| pane.id.clone()).collect::<Vec<_>>();
        let runtimes = pane_ids
            .iter()
            .enumerate()
            .map(|(index, pane_id)| {
                let lines = match index % 4 {
                    0 => vec!["Waiting for approval. Continue?"],
                    1 => vec!["error: command failed"],
                    2 => vec!["STATUS=running | BLOCKER=none | NEXT=finish renderer perf guard"],
                    _ => vec!["done"],
                };
                (pane_id.as_str(), lines)
            })
            .collect::<Vec<_>>();
        let mut app = app_with_panes(panes, runtimes);
        let started = Instant::now();

        for _ in 0..8 {
            let lines = render_grid(&app, 120, 24);
            assert_render_invariants(&lines, 120, 24);
            app.select_next_pane();
        }

        let elapsed = started.elapsed();
        let threshold = if coverage_instrumented() {
            Duration::from_secs(30)
        } else {
            Duration::from_secs(3)
        };
        assert!(
            elapsed < threshold,
            "renderer navigation perf smoke took {elapsed:?}"
        );
    }

    #[test]
    fn output_scroll_key_burst_stays_in_memory_and_below_human_lag_threshold() {
        let log_path = std::env::temp_dir().join(format!(
            "muxboard-scroll-key-burst-{}-{}.log",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock should be after unix epoch")
                .as_nanos()
        ));
        let fake_tmux = fake_tmux_script(
            "scroll-key-burst-no-tmux",
            &format!(
                "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nexit 0\n",
                log_path.display()
            ),
        );
        let mut app = app_with_panes(
            vec![sample_pane("bash")],
            vec![(
                "%1",
                vec![
                    "scroll 01",
                    "scroll 02",
                    "scroll 03",
                    "scroll 04",
                    "scroll 05",
                    "scroll 06",
                    "scroll 07",
                    "scroll 08",
                    "scroll 09",
                    "scroll 10",
                    "scroll 11",
                    "scroll 12",
                    "scroll 13",
                    "scroll 14",
                    "scroll 15",
                    "scroll 16",
                    "scroll 17",
                    "scroll 18",
                    "scroll 19",
                    "scroll 20",
                    "scroll 21",
                    "scroll 22",
                    "scroll 23",
                    "scroll 24",
                    "scroll 25",
                    "scroll 26",
                    "scroll 27",
                    "scroll 28",
                    "scroll 29",
                    "scroll 30",
                    "scroll 31",
                    "scroll 32",
                    "scroll 33",
                    "scroll 34",
                    "scroll 35",
                    "scroll 36",
                ],
            )],
        );
        let runtime = tokio::runtime::Runtime::new().expect("runtime should build");
        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Enter))
            .expect("enter should open output before scroll perf starts");
        use_fake_tmux_for_test(&mut app, fake_tmux);
        let key_count = 320;
        let started = Instant::now();
        runtime.block_on(async {
            for _ in 0..120 {
                handle_key_press(&mut app, KeyCode::Char('k'))
                    .await
                    .expect("older scroll should stay local");
            }
            for _ in 0..120 {
                handle_key_press(&mut app, KeyCode::Char('j'))
                    .await
                    .expect("newer scroll should stay local");
            }
            for _ in 0..20 {
                for key in [
                    KeyCode::PageUp,
                    KeyCode::PageDown,
                    KeyCode::Home,
                    KeyCode::End,
                ] {
                    handle_key_press(&mut app, key)
                        .await
                        .expect("page and endpoint scroll keys should stay local");
                }
            }
        });
        let elapsed = started.elapsed();
        let threshold = if coverage_instrumented() {
            Duration::from_secs(10)
        } else {
            Duration::from_millis(8 * key_count)
        };
        assert!(
            elapsed < threshold,
            "Output scroll key burst should stay under 8ms per key, took {elapsed:?}"
        );
        assert_eq!(app.status_message(), "");
        assert_eq!(
            std::fs::read_to_string(&log_path).unwrap_or_default(),
            "",
            "scrolling selected Output must not invoke tmux capture or focus commands"
        );
        let _ = std::fs::remove_file(log_path);
    }

    #[test]
    fn scroll_render_perf_smoke_stays_smooth_for_focused_details_and_output() {
        fn wrapped_output(prefix: &str) -> Vec<String> {
            (1..=64)
                .map(|index| {
                    format!(
                        "{prefix} {index:02} collecting renderer latency evidence while wrapping enough words to stress the visible scroll path"
                    )
                })
                .collect()
        }

        fn scroll_perf_app(prefix: &str) -> App {
            let output = wrapped_output(prefix);
            let output_refs = output.iter().map(String::as_str).collect::<Vec<_>>();
            app_with_panes(vec![sample_pane("codex")], vec![("%1", output_refs)])
        }

        let runtime = tokio::runtime::Runtime::new().expect("runtime should build");
        let mut rendered_frames = 0_u32;
        let started = Instant::now();

        for (surface, width, height) in [
            ("details-narrow", 82, 18),
            ("details-roomy", 120, 30),
            ("output-narrow", 82, 18),
            ("output-roomy", 120, 30),
        ] {
            let mut app = scroll_perf_app(surface);
            let backend = TestBackend::new(width, height);
            let mut terminal = Terminal::new(backend).expect("test terminal should build");
            if surface.starts_with("output") {
                runtime
                    .block_on(handle_key_press(&mut app, KeyCode::Enter))
                    .expect("enter should open output before scroll render perf starts");
            } else {
                app.cycle_panel_focus();
            }

            let initial = render_grid_on_terminal(&mut terminal, &app);
            assert_render_invariants(&initial, width, height);
            rendered_frames += 1;

            for key in [
                KeyCode::Char('k'),
                KeyCode::Char('k'),
                KeyCode::Char('j'),
                KeyCode::PageUp,
                KeyCode::PageDown,
                KeyCode::Home,
                KeyCode::End,
                KeyCode::Char('k'),
                KeyCode::Char('j'),
                KeyCode::Char('k'),
            ]
            .repeat(4)
            {
                runtime
                    .block_on(handle_key_press(&mut app, key))
                    .expect("focused scroll keys should stay local");
                draw_on_terminal(&mut terminal, &app);
                rendered_frames += 1;
            }
            let final_lines = buffer_grid_lines(terminal.backend().buffer());
            assert_render_invariants(&final_lines, width, height);
        }

        let elapsed = started.elapsed();
        let frame_budget_ms = if github_actions() { 75 } else { 30 };
        let threshold = if coverage_instrumented() {
            Duration::from_secs(60)
        } else {
            Duration::from_millis(frame_budget_ms * u64::from(rendered_frames))
        };
        assert!(
            elapsed < threshold,
            "focused Details/Output scroll render perf should stay under {frame_budget_ms}ms per frame; rendered {rendered_frames} frames in {elapsed:?}"
        );
    }

    #[test]
    fn key_router_handles_modal_text_inputs_without_reaching_tmux() {
        let pane = sample_pane("codex");
        let mut app = app_with_panes(vec![pane], vec![]);
        let runtime = tokio::runtime::Runtime::new().expect("runtime should build");

        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Char('/')))
            .expect("search key should work");
        assert!(app.is_search_input_active());
        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Char('a')))
            .expect("search char should work");
        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Backspace))
            .expect("search backspace should work");
        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Enter))
            .expect("search enter should work");
        assert!(!app.is_search_input_active());

        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Char(':')))
            .expect("command key should work");
        assert!(app.is_command_input_active());
        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Char('x')))
            .expect("command char should work");
        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Backspace))
            .expect("command backspace should work");
        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Esc))
            .expect("command esc should work");
        assert!(!app.is_command_input_active());

        app.open_action_menu();
        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Char('+')))
            .expect("launch action should work");
        assert!(app.is_launch_input_active());
        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Char('x')))
            .expect("launch char should work");
        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Backspace))
            .expect("launch backspace should work");
        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Tab))
            .expect("launch preset cycle should work");
        assert!(
            app.launch_lines()
                .iter()
                .any(|line| line.contains("claude")),
            "{:?}",
            app.launch_lines()
        );
        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Esc))
            .expect("launch esc should work");
        assert!(!app.is_launch_input_active());

        app.toggle_selected_mark();
        app.begin_group_save_input();
        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Char('g')))
            .expect("group char should work");
        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Backspace))
            .expect("group backspace should work");
        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Esc))
            .expect("group esc should work");
        assert!(!app.is_group_input_active());

        app.set_target_groups_for_test(vec![crate::app::TargetGroup {
            name: String::from("triage"),
            members: vec![crate::app::PaneLocator {
                session_name: String::from("demo"),
                window_name: String::from("agents"),
                pane_index: 0,
            }],
        }]);
        app.open_action_menu();
        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Char('l')))
            .expect("fleet picker should open from More");
        assert!(app.is_fleet_picker_active());
        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Char('j')))
            .expect("fleet picker move should work");
        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Enter))
            .expect("fleet picker enter should load");
        assert!(!app.is_fleet_picker_active());
        assert_eq!(
            app.status_message(),
            "Loaded fleet `triage` with 1 pane live."
        );
    }

    #[test]
    fn key_router_handles_help_macro_and_pending_dispatch_modes() {
        let pane = sample_pane("codex");
        let mut app = app_with_panes(vec![pane], vec![]);
        let runtime = tokio::runtime::Runtime::new().expect("runtime should build");

        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Char('?')))
            .expect("help key should work");
        assert!(app.is_help_overlay_active());
        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Char('?')))
            .expect("help close key should work");
        assert!(!app.is_help_overlay_active());

        remember_command_for_test(&mut app, "cargo test");
        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Char('p')))
            .expect("hidden macro assign should be ignored outside Send");
        assert!(!app.is_macro_assign_active());

        app.show_send_view();
        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Char('p')))
            .expect("macro assign should open");
        assert!(app.is_macro_assign_active());
        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Char('1')))
            .expect("macro slot should assign");
        assert!(!app.is_macro_assign_active());

        let mut first = sample_pane("codex");
        first.id = String::from("%1");
        let mut second = sample_pane("claude");
        second.id = String::from("%2");
        second.active = false;
        second.pane_index = 1;
        let mut app = app_with_panes(vec![first, second], vec![]);
        app.toggle_selected_mark();
        app.select_next_pane();
        app.toggle_selected_mark();
        remember_command_for_test(&mut app, "cargo test");
        app.begin_command_input();
        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Char(']')))
            .expect("recent repeat should stage without typing a bracket");
        assert!(!app.is_command_input_active());
        assert!(app.has_pending_dispatch());
        assert!(app.status_message().contains("Review send `cargo test`"));
        assert!(app.status_message().contains("list (2 panes)"));

        let mut app =
            app_from_view_model_fixture(&view_fixture("confirm_dispatch_header_and_footer"));
        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Esc))
            .expect("pending dispatch esc should cancel");
        assert!(!app.has_pending_dispatch());
    }

    #[test]
    fn key_router_handles_top_level_navigation_and_targeting() {
        let mut first = sample_pane("codex");
        first.id = String::from("%1");
        let mut second = sample_pane("claude");
        second.id = String::from("%2");
        second.active = false;
        second.pane_index = 1;
        let mut app = app_with_panes(vec![first, second], vec![]);
        let runtime = tokio::runtime::Runtime::new().expect("runtime should build");

        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Char('J')))
            .expect("move down should work");
        assert_eq!(app.selected_pane_lines()[0], "demo/agents");

        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Char('K')))
            .expect("move up should work");
        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Char(' ')))
            .expect("mark should work");
        assert!(app.board_title(8).contains("send list 1 pane"));

        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Char('x')))
            .expect("clear marks should work");
        assert!(!app.board_title(8).contains("in send list"));

        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Tab))
            .expect("panel focus should work");
        assert_eq!(app.context_panel_title(), "Details");
        assert!(app.is_details_panel_focused());

        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Char('.')))
            .expect("more should open");
        assert!(app.is_action_menu_active());

        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Esc))
            .expect("esc closes more");
        assert!(!app.is_action_menu_active());

        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Esc))
            .expect("top-level esc should be safe");
        assert!(!app.should_quit());

        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Char('q')))
            .expect("quit key should work");
        assert!(app.should_quit());
    }

    #[test]
    fn command_center_escape_returns_to_details_and_footer_advertises_back() {
        let pane = sample_pane("codex");
        let mut app = app_with_panes(vec![pane], vec![]);
        let runtime = tokio::runtime::Runtime::new().expect("runtime should build");

        app.show_command_center();
        assert_eq!(app.context_panel_title(), "Command Center");
        assert!(app.is_command_center_active());

        let lines = render_grid(&app, 80, 20);
        let screen = screen_text(&lines);
        let footer = lines.last().expect("footer should render");
        assert!(footer.contains("Esc back"), "{screen}");
        assert!(footer.contains("Q quit"), "{screen}");

        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Esc))
            .expect("escape should close Command Center");

        assert_eq!(app.context_panel_title(), "Details");
        assert!(!app.is_command_center_active());
        assert!(app.is_details_panel_focused());
        assert!(!app.should_quit());

        let restored = screen_text(&render_grid(&app, 80, 20));
        assert!(restored.contains("Details"), "{restored}");
        assert!(!restored.contains("Command Center"), "{restored}");
    }

    #[test]
    fn browse_escape_returns_to_details_and_footer_advertises_back() {
        let pane = sample_pane("codex");
        let mut app = app_with_panes(vec![pane], vec![]);
        let runtime = tokio::runtime::Runtime::new().expect("runtime should build");

        app.show_browse_view();
        assert!(app.context_panel_title().starts_with("Browse"));
        assert!(app.is_browse_view_active());

        let lines = render_grid(&app, 80, 20);
        let screen = screen_text(&lines);
        let footer = lines.last().expect("footer should render");
        assert!(footer.contains("Esc back"), "{screen}");
        assert!(footer.contains("Q quit"), "{screen}");

        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Esc))
            .expect("escape should close Browse");

        assert_eq!(app.context_panel_title(), "Details");
        assert!(!app.is_browse_view_active());
        assert!(app.is_details_panel_focused());
        assert!(!app.should_quit());

        let restored = screen_text(&render_grid(&app, 80, 20));
        assert!(restored.contains("Details"), "{restored}");
        assert!(!restored.contains("Browse"), "{restored}");
    }

    #[test]
    fn key_router_only_runs_repeat_and_macro_when_send_shortcuts_are_visible() {
        let mut first = sample_pane("codex");
        first.id = String::from("%1");
        let mut second = sample_pane("claude");
        second.id = String::from("%2");
        second.active = false;
        second.pane_index = 1;
        let mut app = app_with_panes(vec![first, second], vec![]);
        app.toggle_selected_mark();
        app.select_next_pane();
        app.toggle_selected_mark();
        remember_command_for_test(&mut app, "cargo test");
        app.begin_macro_assign();
        app.assign_recent_command_to_slot(0);
        app.cycle_context_pane();
        app.cycle_context_pane();
        app.cycle_context_pane();
        let runtime = tokio::runtime::Runtime::new().expect("runtime should build");

        assert!(!app.command_shortcuts_are_visible());
        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Char(']')))
            .expect("invisible repeat should not dispatch");
        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Char('1')))
            .expect("invisible macro should not dispatch");
        assert!(!app.has_pending_dispatch());

        app.cycle_context_pane();
        app.cycle_context_pane();
        assert!(app.command_shortcuts_are_visible());
        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Char(']')))
            .expect("visible repeat should stage");
        assert!(app.has_pending_dispatch());
        assert!(app.status_message().contains("Review send `cargo test`"));

        app.cancel_pending_dispatch();
        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Char('1')))
            .expect("visible macro should stage");
        assert!(app.has_pending_dispatch());
        assert!(app.status_message().contains("Review send `cargo test`"));
    }

    #[test]
    fn key_router_handles_action_menu_pure_actions_and_dismissal() {
        let pane = sample_pane("codex");
        let mut app = app_with_panes(vec![pane], vec![]);
        let runtime = tokio::runtime::Runtime::new().expect("runtime should build");

        for key in [
            KeyCode::Char('t'),
            KeyCode::Char('f'),
            KeyCode::Enter,
            KeyCode::Char('['),
            KeyCode::Char(']'),
            KeyCode::Char('b'),
            KeyCode::Char('m'),
            KeyCode::Char('o'),
            KeyCode::Char('v'),
            KeyCode::Char('h'),
            KeyCode::Char('p'),
        ] {
            app.open_action_menu();
            runtime
                .block_on(handle_key_press(&mut app, key))
                .unwrap_or_else(|error| panic!("action key {key:?} should work: {error}"));
            assert!(!app.is_action_menu_active(), "{key:?}");
        }

        assert_eq!(app.context_panel_title(), "Command Center");
        assert!(app.is_details_panel_focused());

        app.open_action_menu();
        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Char('[')))
            .expect("browse view should open from More");
        assert_eq!(app.context_panel_title(), "Browse");
        assert!(app.is_details_panel_focused());

        app.open_action_menu();
        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Char(':')))
            .expect("send view should open from More");
        assert_eq!(app.context_panel_title(), "Send");
        assert!(app.is_details_panel_focused());

        app.open_action_menu();
        let before_group_save = render_grid(&app, 104, 20);
        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Char('9')))
            .expect("unlisted key should be inert");
        assert!(app.is_action_menu_active());
        assert_eq!(
            render_grid(&app, 104, 20),
            before_group_save,
            "unlisted key must not mutate the More menu"
        );
    }

    #[test]
    fn key_router_covers_safe_no_target_async_actions() {
        let mut app = app_with_panes(Vec::new(), vec![]);
        let runtime = tokio::runtime::Runtime::new().expect("runtime should build");

        app.open_action_menu();
        let empty_more = render_grid(&app, 104, 20);
        for key in [
            KeyCode::Char('z'),
            KeyCode::Char('e'),
            KeyCode::Char('y'),
            KeyCode::Char('n'),
            KeyCode::Char('i'),
        ] {
            runtime
                .block_on(handle_key_press(&mut app, key))
                .unwrap_or_else(|error| panic!("safe action key {key:?} should work: {error}"));
            assert!(app.is_action_menu_active(), "{key:?}");
            assert_eq!(
                render_grid(&app, 104, 20),
                empty_more,
                "unlisted empty More key {key:?} must not mutate the menu"
            );
        }
        app.close_action_menu();

        for key in [
            KeyCode::Char('a'),
            KeyCode::Enter,
            KeyCode::Char('g'),
            KeyCode::Char(']'),
            KeyCode::Char('s'),
        ] {
            runtime
                .block_on(handle_key_press(&mut app, key))
                .unwrap_or_else(|error| panic!("safe top-level key {key:?} should work: {error}"));
        }

        assert!(!app.status_message().is_empty());

        app.begin_macro_assign();
        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Esc))
            .expect("macro escape should be safe without recent command");
    }

    #[test]
    fn usability_action_contract_tmux_failures_stay_in_muxboard() {
        let failing_tmux = fake_tmux_script(
            "action-failure-stays-visible",
            r#"#!/bin/sh
if [ "$1" = "send-keys" ]; then
  echo "permission denied by tmux hook" >&2
  exit 1
fi

printf 'unexpected tmux args: %s\n' "$*" >&2
exit 64
"#,
        );
        let runtime = tokio::runtime::Runtime::new().expect("runtime should build");

        let mut app = app_with_panes(vec![sample_pane("codex")], vec![]);
        use_fake_tmux_for_test(&mut app, failing_tmux.clone());
        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Char(':')))
            .expect("send should open");
        for ch in "echo hi".chars() {
            runtime
                .block_on(handle_key_press(&mut app, KeyCode::Char(ch)))
                .expect("typing should stay in muxboard");
        }
        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Enter))
            .expect("tmux send failure should be visible, not fatal");

        assert_eq!(
            app.status_message(),
            "Action failed: permission denied by tmux hook."
        );
        assert!(!app.status_message().contains("tmux command failed"));
        let screen = screen_text(&render_grid(&app, 100, 18));
        assert!(
            screen.contains("Action failed: permission denied"),
            "{screen}"
        );
        assert!(!screen.contains("tmux command failed"), "{screen}");
        assert!(!app.should_quit());
        assert_eq!(app.snapshot().pane_count(), 1);

        let mut waiting = app_with_panes(
            vec![sample_pane("codex")],
            vec![("%1", vec!["Press Enter to continue."])],
        );
        use_fake_tmux_for_test(&mut waiting, failing_tmux);
        runtime
            .block_on(handle_key_press(&mut waiting, KeyCode::Char('a')))
            .expect("smart Enter failure should be visible, not fatal");

        assert_eq!(
            waiting.status_message(),
            "Action failed: permission denied by tmux hook."
        );
        assert!(!waiting.status_message().contains("tmux command failed"));
        let screen = screen_text(&render_grid(&waiting, 100, 18));
        assert!(
            screen.contains("Action failed: permission denied"),
            "{screen}"
        );
        assert!(!screen.contains("tmux command failed"), "{screen}");
        assert!(!waiting.should_quit());
        assert_eq!(waiting.snapshot().pane_count(), 1);
    }

    #[test]
    fn usability_action_contract_close_after_jump_only_exits_on_jump() {
        let fake_tmux = fake_tmux_script(
            "close-after-jump-key",
            r#"#!/bin/sh
if [ "$1" = "display-message" ]; then
  echo '/dev/ttys999'
fi
exit 0
"#,
        );
        let runtime = tokio::runtime::Runtime::new().expect("runtime should build");

        let mut drawer = app_with_panes(vec![sample_pane("codex")], vec![("%1", vec!["work"])]);
        use_fake_tmux_for_test(&mut drawer, fake_tmux.clone());
        drawer.set_close_after_jump_for_test(true);
        runtime
            .block_on(handle_key_press(&mut drawer, KeyCode::Enter))
            .expect("opening output should not close drawer mode");
        assert!(!drawer.should_quit());
        runtime
            .block_on(handle_key_press(&mut drawer, KeyCode::Esc))
            .expect("escape should return to Fleet without closing drawer mode");
        assert!(!drawer.should_quit());
        runtime
            .block_on(handle_key_press(&mut drawer, KeyCode::Char('g')))
            .expect("jump should close drawer mode after showing the pane");
        assert!(drawer.should_quit());

        let mut center = app_with_panes(vec![sample_pane("codex")], vec![]);
        use_fake_tmux_for_test(&mut center, fake_tmux);
        runtime
            .block_on(handle_key_press(&mut center, KeyCode::Char('g')))
            .expect("default jump should keep muxboard open");
        assert!(!center.should_quit());
        assert!(
            center
                .status_message()
                .contains("Muxboard is still running"),
            "{}",
            center.status_message()
        );
    }

    #[test]
    fn usability_action_contract_peek_prefix_key_quits_only_after_full_toggle_sequence() {
        let runtime = tokio::runtime::Runtime::new().expect("runtime should build");
        let toggle_keys = PeekToggleKeys {
            prefixes: vec![KeyPattern::Ctrl('b'), KeyPattern::Ctrl('a')],
            key: KeyPattern::Char('P'),
        };
        let mut prefix_pending = false;
        let mut app = app_with_panes(vec![sample_pane("codex")], vec![("%1", vec!["work"])]);

        runtime
            .block_on(handle_key_event(
                &mut app,
                KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL),
                Some(&toggle_keys),
                &mut prefix_pending,
            ))
            .expect("tmux prefix should be captured inside peek mode");
        assert!(!app.should_quit());
        assert!(prefix_pending);

        runtime
            .block_on(handle_key_event(
                &mut app,
                KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE),
                Some(&toggle_keys),
                &mut prefix_pending,
            ))
            .expect("non-toggle keys should keep working after a stray prefix");
        assert!(!app.should_quit());
        assert!(!prefix_pending);

        runtime
            .block_on(handle_key_event(
                &mut app,
                KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL),
                Some(&toggle_keys),
                &mut prefix_pending,
            ))
            .expect("tmux prefix should be captured before the drawer key");
        runtime
            .block_on(handle_key_event(
                &mut app,
                KeyEvent::new(KeyCode::Char('P'), KeyModifiers::SHIFT),
                Some(&toggle_keys),
                &mut prefix_pending,
            ))
            .expect("drawer key should close the peek drawer");

        assert!(app.should_quit());
        assert!(!prefix_pending);

        let mut esc_app = app_with_panes(vec![sample_pane("codex")], vec![("%1", vec!["work"])]);
        prefix_pending = true;
        runtime
            .block_on(handle_key_event(
                &mut esc_app,
                KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
                Some(&toggle_keys),
                &mut prefix_pending,
            ))
            .expect("escape should close the peek drawer");
        assert!(esc_app.should_quit());
        assert!(!prefix_pending);

        let mut literal_prefix_app =
            app_with_panes(vec![sample_pane("codex")], vec![("%1", vec!["work"])]);
        runtime
            .block_on(handle_key_event(
                &mut literal_prefix_app,
                KeyEvent::new(KeyCode::Char('\u{2}'), KeyModifiers::NONE),
                Some(&toggle_keys),
                &mut prefix_pending,
            ))
            .expect("literal control prefix bytes should be captured");
        runtime
            .block_on(handle_key_event(
                &mut literal_prefix_app,
                KeyEvent::new(KeyCode::Char('P'), KeyModifiers::SHIFT),
                Some(&toggle_keys),
                &mut prefix_pending,
            ))
            .expect("drawer key should close after a literal prefix byte");
        assert!(literal_prefix_app.should_quit());
        assert!(!prefix_pending);

        let mut shifted_lowercase_app =
            app_with_panes(vec![sample_pane("codex")], vec![("%1", vec!["work"])]);
        runtime
            .block_on(handle_key_event(
                &mut shifted_lowercase_app,
                KeyEvent::new(KeyCode::Char('\u{2}'), KeyModifiers::NONE),
                Some(&toggle_keys),
                &mut prefix_pending,
            ))
            .expect("literal control prefix bytes should be captured");
        runtime
            .block_on(handle_key_event(
                &mut shifted_lowercase_app,
                KeyEvent::new(KeyCode::Char('p'), KeyModifiers::SHIFT),
                Some(&toggle_keys),
                &mut prefix_pending,
            ))
            .expect("shifted lowercase drawer key should close the peek drawer");
        assert!(shifted_lowercase_app.should_quit());
        assert!(!prefix_pending);

        let mut prefix2_app =
            app_with_panes(vec![sample_pane("codex")], vec![("%1", vec!["work"])]);
        runtime
            .block_on(handle_key_event(
                &mut prefix2_app,
                KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL),
                Some(&toggle_keys),
                &mut prefix_pending,
            ))
            .expect("tmux prefix2 should also be captured inside peek mode");
        assert!(!prefix2_app.should_quit());
        assert!(prefix_pending);
        runtime
            .block_on(handle_key_event(
                &mut prefix2_app,
                KeyEvent::new(KeyCode::Char('P'), KeyModifiers::SHIFT),
                Some(&toggle_keys),
                &mut prefix_pending,
            ))
            .expect("drawer key should close after prefix2");
        assert!(prefix2_app.should_quit());
        assert!(!prefix_pending);
    }

    #[test]
    fn usability_action_contract_terminal_enter_variants_apply_text_inputs() {
        assert_eq!(
            normalized_app_key_code(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL)),
            KeyCode::Enter
        );
        assert_eq!(
            normalized_app_key_code(KeyEvent::new(KeyCode::Char('m'), KeyModifiers::CONTROL)),
            KeyCode::Enter
        );
        assert_eq!(
            normalized_app_key_code(KeyEvent::new(KeyCode::Char('\n'), KeyModifiers::NONE)),
            KeyCode::Enter
        );

        let runtime = tokio::runtime::Runtime::new().expect("runtime should build");
        let mut app = app_with_panes(vec![sample_pane("codex")], vec![("%1", vec!["work"])]);
        let mut prefix_pending = false;

        runtime
            .block_on(handle_key_event(
                &mut app,
                KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE),
                None,
                &mut prefix_pending,
            ))
            .expect("slash should open search");
        for ch in "prompt".chars() {
            runtime
                .block_on(handle_key_event(
                    &mut app,
                    KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE),
                    None,
                    &mut prefix_pending,
                ))
                .expect("search text should type");
        }
        runtime
            .block_on(handle_key_event(
                &mut app,
                KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL),
                None,
                &mut prefix_pending,
            ))
            .expect("tmux Enter as Ctrl-J should apply search");

        let screen = screen_text(&render_grid(&app, 100, 18));
        assert!(!app.is_search_input_active());
        assert_eq!(app.status_message(), "Filtering panes by `prompt`.");
        assert!(!screen.contains("promptj"), "{screen}");
    }

    #[test]
    fn board_layout_mode_adapts_to_width() {
        assert_eq!(BoardLayoutMode::for_width(120), BoardLayoutMode::Full);
        assert_eq!(BoardLayoutMode::for_width(80), BoardLayoutMode::Standard);
        assert_eq!(BoardLayoutMode::for_width(60), BoardLayoutMode::Compact);
    }

    #[test]
    fn body_layout_stacks_on_narrow_terminals() {
        assert_eq!(
            BodyLayoutMode::for_area(70, 14, false, false, false),
            BodyLayoutMode::Stack
        );
        assert_eq!(
            BodyLayoutMode::for_area(100, 30, false, false, false),
            BodyLayoutMode::Stack
        );
        assert_eq!(
            BodyLayoutMode::for_area(119, 24, false, false, false),
            BodyLayoutMode::Stack
        );
        assert_eq!(
            BodyLayoutMode::for_area(120, 24, false, false, false),
            BodyLayoutMode::SplitColumns
        );
        assert_eq!(
            BodyLayoutMode::for_area(132, 24, false, false, false),
            BodyLayoutMode::SplitColumns
        );
        assert_eq!(
            BodyLayoutMode::for_area(140, 30, false, false, false),
            BodyLayoutMode::SplitColumns
        );
        assert_eq!(
            BodyLayoutMode::for_area(124, 24, true, true, false),
            BodyLayoutMode::Stack
        );
    }

    #[test]
    fn auto_layout_keeps_geometry_stable_when_details_focus_changes() {
        let mut app = app_with_panes(vec![sample_pane("codex")], vec![("%1", vec!["ready"])]);
        let profile = LayoutProfile {
            header_height: 1,
            footer_height: 1,
            board_percent: 55,
            context_percent: 45,
            stacked_board_percent: 54,
            stacked_context_percent: 46,
        };

        let split = ResolvedBodyLayout::for_app(&app, Rect::new(0, 0, 140, 22), profile);
        assert_eq!(split.mode, BodyLayoutMode::SplitColumns);
        assert_eq!((split.board_percent, split.context_percent), (55, 45));

        app.cycle_panel_focus();
        let focused_split = ResolvedBodyLayout::for_app(&app, Rect::new(0, 0, 140, 22), profile);
        assert_eq!(focused_split.mode, BodyLayoutMode::SplitColumns);
        assert_eq!(
            (focused_split.board_percent, focused_split.context_percent),
            (55, 45)
        );

        let focused_stack = ResolvedBodyLayout::for_app(&app, Rect::new(0, 0, 119, 22), profile);
        assert_eq!(focused_stack.mode, BodyLayoutMode::Stack);
        assert_eq!(
            (focused_stack.board_percent, focused_stack.context_percent),
            (54, 46)
        );

        app.set_layout_preset_for_test(LayoutPreset::Horizontal);
        let forced_split = ResolvedBodyLayout::for_app(&app, Rect::new(0, 0, 120, 22), profile);
        assert_eq!(forced_split.mode, BodyLayoutMode::SplitColumns);

        app.set_layout_preset_for_test(LayoutPreset::Vertical);
        let forced_stack = ResolvedBodyLayout::for_app(&app, Rect::new(0, 0, 160, 22), profile);
        assert_eq!(forced_stack.mode, BodyLayoutMode::Stack);
    }

    #[test]
    fn usability_tab_focus_changes_style_not_panel_geometry() {
        let mut simple = app_with_panes(vec![sample_pane("codex")], vec![("%1", vec!["ready"])]);
        for &(width, height) in &[(100, 24), (120, 30), (132, 24)] {
            simple.set_layout_preset_for_test(LayoutPreset::Auto);
            let before = render_grid(&simple, width, height);
            simple.cycle_panel_focus();
            let after = render_grid(&simple, width, height);
            simple.cycle_panel_focus();

            assert_render_invariants(&before, width, height);
            assert_render_invariants(&after, width, height);
            assert_eq!(
                text_position(&before, "Fleet"),
                text_position(&after, "Fleet")
            );
            assert_eq!(
                text_position(&before, "Details"),
                text_position(&after, "Details")
            );
            assert_eq!(
                panel_border_signature(&before),
                panel_border_signature(&after),
                "Tab focus must not move panel borders at {width}x{height}:\nbefore:\n{}\n\nafter:\n{}",
                screen_text(&before),
                screen_text(&after)
            );
        }

        let mut content_heavy = app_with_panes(
            vec![sample_pane("codex")],
            vec![(
                "%1",
                vec![
                    "step 01 plan",
                    "step 02 inspect",
                    "step 03 patch",
                    "step 04 test",
                    "step 05 render",
                    "step 06 inspect",
                    "step 07 iterate",
                    "step 08 verify",
                    "step 09 document",
                    "step 10 commit",
                    "step 11 audit",
                    "step 12 done",
                ],
            )],
        );
        let before = render_grid(&content_heavy, 124, 24);
        content_heavy.cycle_panel_focus();
        let after = render_grid(&content_heavy, 124, 24);
        assert_eq!(
            text_position(&before, "Fleet"),
            text_position(&after, "Fleet")
        );
        assert_eq!(
            text_position(&before, "Details"),
            text_position(&after, "Details")
        );
        assert_eq!(
            panel_border_signature(&before),
            panel_border_signature(&after)
        );
    }

    #[test]
    fn sub_120_terminals_stack_to_reduce_cramped_copy() {
        let mut pane = sample_pane("node");
        pane.window_name = String::from("codex");
        let app = app_with_panes(
            vec![pane],
            vec![(
                "%1",
                vec![
                    "STATUS=running | BLOCKER=none | NEXT=investigate renderer layout constraints and preserve the meaningful latest status",
                    "Running renderer-level usability pass across cramped terminal sizes",
                ],
            )],
        );

        let lines = render_grid(&app, 116, 24);
        let screen = screen_text(&lines);

        assert_render_invariants(&lines, 116, 24);
        assert!(
            line_index(&lines, "Details") > line_index(&lines, "Fleet"),
            "sub-120 layouts should stack Fleet over Details instead of forcing two cramped columns:\n{screen}"
        );
        assert!(
            screen.contains("investigate renderer layout constraints"),
            "{screen}"
        );
        assert!(
            screen.contains("run renderer-level usability pass"),
            "{screen}"
        );
    }

    #[test]
    fn dashboard_auto_layout_matrix_chooses_readable_split_or_stack() {
        let app = app_with_panes(vec![sample_pane("codex")], vec![("%1", vec!["ready"])]);

        for (width, height, expected_stack) in [
            (80, 16, true),
            (100, 20, true),
            (116, 24, true),
            (120, 24, false),
            (132, 26, false),
            (140, 24, false),
            (160, 24, false),
        ] {
            let lines = render_grid(&app, width, height);
            let screen = screen_text(&lines);
            let fleet = line_index(&lines, "Fleet");
            let details = line_index(&lines, "Details");

            assert_render_invariants(&lines, width, height);
            assert_eq!(
                details > fleet,
                expected_stack,
                "{width}x{height} should {}:\n{screen}",
                if expected_stack { "stack" } else { "split" }
            );
        }
    }

    #[test]
    fn dashboard_layout_override_switches_between_side_by_side_and_stacked() {
        let mut app = app_with_panes(vec![sample_pane("codex")], vec![("%1", vec!["ready"])]);

        app.set_layout_preset_for_test(LayoutPreset::Horizontal);
        let horizontal = render_grid(&app, 120, 24);
        let horizontal_screen = screen_text(&horizontal);
        assert_render_invariants(&horizontal, 120, 24);
        assert_eq!(
            line_index(&horizontal, "Details"),
            line_index(&horizontal, "Fleet"),
            "Horizontal should force side-by-side panes:\n{horizontal_screen}"
        );

        app.set_layout_preset_for_test(LayoutPreset::Vertical);
        let vertical = render_grid(&app, 160, 24);
        let vertical_screen = screen_text(&vertical);
        assert_render_invariants(&vertical, 160, 24);
        assert!(
            line_index(&vertical, "Details") > line_index(&vertical, "Fleet"),
            "Vertical should force stacked panes:\n{vertical_screen}"
        );
    }

    #[test]
    fn auto_layout_uses_density_focus_and_content_pressure_for_ratios() {
        let profile = LayoutProfile {
            header_height: 1,
            footer_height: 1,
            board_percent: 55,
            context_percent: 45,
            stacked_board_percent: 54,
            stacked_context_percent: 46,
        };
        let mut roomy = app_with_panes(vec![sample_pane("codex")], vec![("%1", vec!["ready"])]);
        let normal = ResolvedBodyLayout::for_app(&roomy, Rect::new(0, 0, 160, 22), profile);
        assert_eq!(normal.mode, BodyLayoutMode::SplitColumns);
        assert_eq!((normal.board_percent, normal.context_percent), (55, 45));

        roomy.cycle_panel_focus();
        let focused = ResolvedBodyLayout::for_app(&roomy, Rect::new(0, 0, 160, 22), profile);
        assert_eq!(focused.mode, BodyLayoutMode::SplitColumns);
        assert_eq!((focused.board_percent, focused.context_percent), (55, 45));

        let mut content_heavy = app_with_panes(
            vec![sample_pane("codex")],
            vec![(
                "%1",
                vec![
                    "step 01 plan",
                    "step 02 inspect",
                    "step 03 patch",
                    "step 04 test",
                    "step 05 render",
                    "step 06 inspect",
                    "step 07 iterate",
                    "step 08 verify",
                    "step 09 document",
                    "step 10 commit",
                    "step 11 audit",
                    "step 12 done",
                ],
            )],
        );
        content_heavy.cycle_panel_focus();
        let pressured =
            ResolvedBodyLayout::for_app(&content_heavy, Rect::new(0, 0, 140, 14), profile);
        assert_eq!(pressured.mode, BodyLayoutMode::SplitColumns);
        assert_eq!(
            (pressured.board_percent, pressured.context_percent),
            (48, 52)
        );

        let dense_panes = (0..24)
            .map(|index| {
                let mut pane = sample_pane("codex");
                pane.id = format!("%{index}");
                pane.window_id = format!("@{index}");
                pane.window_name = format!("agent-{index:02}");
                pane.pane_index = index;
                pane.active = index == 0;
                pane
            })
            .collect::<Vec<_>>();
        let dense = app_with_panes(dense_panes, vec![]);
        let dense_layout = ResolvedBodyLayout::for_app(&dense, Rect::new(0, 0, 160, 22), profile);
        assert_eq!(dense_layout.mode, BodyLayoutMode::SplitColumns);
        assert_eq!(
            (dense_layout.board_percent, dense_layout.context_percent),
            (58, 42)
        );
    }

    #[test]
    fn content_heavy_details_can_stack_at_medium_width_without_forcing_all_120s_to_stack() {
        let mut pane = sample_pane("codex");
        pane.current_command = String::from("codex");
        let mut app = app_with_panes(
            vec![pane],
            vec![(
                "%1",
                vec![
                    "STATUS=running | BLOCKER=none | NEXT=review renderer hierarchy and ratio allocation before changing code",
                    "collect current footer",
                    "inspect help copy",
                    "patch layout resolver",
                    "run renderer xray",
                    "review golden",
                    "commit",
                    "audit",
                    "done",
                ],
            )],
        );
        app.cycle_panel_focus();

        let medium = render_grid(&app, 124, 24);
        let screen = screen_text(&medium);
        assert_render_invariants(&medium, 124, 24);
        assert!(
            line_index(&medium, "Details") > line_index(&medium, "Fleet"),
            "content-heavy Details should stack at medium width instead of compressing both panels:\n{screen}"
        );

        let simple = app_with_panes(vec![sample_pane("codex")], vec![("%1", vec!["ready"])]);
        let simple_lines = render_grid(&simple, 120, 24);
        let simple_screen = screen_text(&simple_lines);
        assert_render_invariants(&simple_lines, 120, 24);
        assert_eq!(
            line_index(&simple_lines, "Details"),
            line_index(&simple_lines, "Fleet"),
            "simple 120-column dashboards should remain side by side:\n{simple_screen}"
        );
    }

    #[test]
    fn usability_layout_presets_keep_wayfinding_and_actions_visible() {
        let mut app = app_with_panes(vec![sample_pane("codex")], vec![("%1", vec!["ready"])]);

        for preset in [
            LayoutPreset::Auto,
            LayoutPreset::Horizontal,
            LayoutPreset::Vertical,
        ] {
            app.set_layout_preset_for_test(preset);
            for (width, height) in [(120, 20), (160, 24)] {
                let lines = render_grid(&app, width, height);
                let screen = screen_text(&lines);
                let footer = lines.last().expect("footer should render");
                assert_render_invariants(&lines, width, height);
                assert_no_low_value_copy("layout preset", &lines);
                for term in ["muxboard", "Fleet", "Details", "demo/agents", "ready"] {
                    assert!(
                        screen.contains(term),
                        "{preset:?} at {width}x{height} missing `{term}`:\n{screen}"
                    );
                }
                for term in ["J/K move", "Enter output", ": send", "L layout", ". more"] {
                    assert!(
                        footer.contains(term),
                        "{preset:?} at {width}x{height} footer missing `{term}`:\n{screen}"
                    );
                }
            }
        }
    }

    #[test]
    fn layout_control_is_discoverable_from_footer_help_and_more() {
        let mut app = app_with_panes(vec![sample_pane("codex")], vec![("%1", vec!["ready"])]);
        let runtime = tokio::runtime::Runtime::new().expect("runtime should build");
        let press = |app: &mut App, key| {
            runtime
                .block_on(handle_key_press(app, key))
                .expect("layout key should route")
        };

        let initial = render_grid(&app, 120, 20);
        let initial_screen = screen_text(&initial);
        let footer = initial.last().expect("footer should render");
        assert_render_invariants(&initial, 120, 20);
        assert!(
            footer.contains("L layout"),
            "layout switching must be discoverable without opening More:\n{initial_screen}"
        );

        press(&mut app, KeyCode::Char('L'));
        assert_eq!(app.layout_preset(), LayoutPreset::Horizontal);
        assert_eq!(app.status_message(), "Layout: side by side.");

        press(&mut app, KeyCode::Char('?'));
        let help_lines = render_grid(&app, 120, 20);
        let help = screen_text(&help_lines);
        assert_render_invariants(&help_lines, 120, 20);
        assert!(app.is_help_overlay_active());
        assert!(
            help.contains("L layout"),
            "Help must expose the layout key, not just More:\n{help}"
        );

        press(&mut app, KeyCode::Char('L'));
        assert!(!app.is_help_overlay_active());
        assert_eq!(app.layout_preset(), LayoutPreset::Vertical);
        assert_eq!(app.status_message(), "Layout: stacked.");

        press(&mut app, KeyCode::Char('.'));
        let more_lines = render_grid(&app, 130, 36);
        let more = screen_text(&more_lines);
        assert_render_invariants(&more_lines, 130, 36);
        assert!(more.contains("L layout: stacked"), "{more}");
        press(&mut app, KeyCode::Char('L'));
        assert!(!app.is_action_menu_active());
        assert_eq!(app.layout_preset(), LayoutPreset::Auto);
        assert_eq!(app.status_message(), "Layout: auto.");
    }

    #[test]
    fn board_headers_shrink_with_compact_widths() {
        assert_eq!(
            board_headers(BoardLayoutMode::Compact),
            vec!["", "Where", "Latest"]
        );
    }

    #[test]
    fn board_constraints_give_latest_the_flexible_column() {
        assert_eq!(
            board_constraints(BoardLayoutMode::Standard, BOARD_LOCATION_MIN_WIDTH),
            vec![
                Constraint::Length(2),
                Constraint::Length(BOARD_LOCATION_MIN_WIDTH),
                Constraint::Max(9),
                Constraint::Fill(1),
            ]
        );
        assert_eq!(
            board_constraints(BoardLayoutMode::Compact, BOARD_LOCATION_MIN_WIDTH),
            vec![
                Constraint::Length(2),
                Constraint::Length(BOARD_LOCATION_MIN_WIDTH),
                Constraint::Fill(1),
            ]
        );
        assert_eq!(
            board_constraints(BoardLayoutMode::Full, BOARD_LOCATION_MIN_WIDTH),
            vec![
                Constraint::Length(2),
                Constraint::Length(BOARD_LOCATION_MIN_WIDTH),
                Constraint::Max(8),
                Constraint::Max(9),
                Constraint::Fill(1),
            ]
        );
    }

    #[test]
    fn board_location_width_grows_only_for_visible_labels_that_need_it() {
        let mut short = board_row("demo/agents", "idle", "ready");
        short.selected = true;
        let plain = board_row("muxdog/claude", "error", "command failed");
        let long = board_row("very-long-session-name/worker", "running", "ship fix");

        assert_eq!(board_location_width(&[short]), BOARD_LOCATION_MIN_WIDTH);
        assert_eq!(
            board_location_width(&[plain]),
            "muxdog/claude".chars().count() as u16
        );
        assert_eq!(board_location_width(&[long]), BOARD_LOCATION_MAX_WIDTH);
    }

    #[test]
    fn zero_row_board_capacity_keeps_layout_recoverable() {
        let app = app_with_panes(
            vec![sample_pane("codex")],
            vec![("%1", vec!["Waiting for approval. Continue?"])],
        );
        let (limit, rows, location_width, latest_width) =
            board_rows_for_capacity(&app, 0, BoardLayoutMode::Standard, 80);

        assert_eq!(limit, 0);
        assert!(rows.is_empty());
        assert_eq!(location_width, BOARD_LOCATION_MIN_WIDTH);
        assert!(latest_width > 0);

        let lines = render_grid(&app, 80, 6);
        let screen = screen_text(&lines);
        assert_render_invariants(&lines, 80, 6);
        assert!(screen.contains("Fleet"), "{screen}");
        assert!(screen.contains("Details"), "{screen}");
    }

    #[test]
    fn truncate_cell_shortens_long_board_values() {
        assert_eq!(truncate_cell("very/long/session/name", 10), "very/lo...");
    }

    #[test]
    fn truncate_location_cell_preserves_duplicate_suffix_when_possible() {
        assert_eq!(truncate_location_cell("demo/agents#12", 12), "demo/a...#12");
        assert_eq!(
            truncate_location_cell("very-long-session/ridiculously-long-window#3", 12),
            "very-lo...#3"
        );
    }

    #[test]
    fn top_centered_rect_keeps_overlay_near_the_top() {
        let rect = top_centered_rect(Rect::new(0, 1, 100, 18), 50, 8, 1);
        assert_eq!(rect.y, 2);
        assert_eq!(rect.width, 50);
        assert_eq!(rect.height, 8);
    }

    #[test]
    fn output_overlay_prefers_more_width_than_actions() {
        let body = Rect::new(0, 1, 120, 20);
        let lines = vec![
            String::from("download dependencies"),
            String::from("compile crate"),
        ];

        let output = overlay_rect(body, "Output", &lines);
        let actions = overlay_rect(body, "More", &lines);

        assert!(output.width > actions.width);
        assert_eq!(output.y, 2);
        assert_eq!(actions.y, 2);
    }

    #[test]
    fn usability_output_overlay_hugs_summary_only_content_without_blank_chrome() {
        let fixture = panel_fixture("live_tail_with_summary_and_raw_tail");
        let app = app_from_panel_fixture(&fixture);
        let (title, raw_lines) = app
            .overlay_panel()
            .expect("output overlay should be visible");
        let body = body_rect(100, 16);
        let rect = overlay_rect(body, &title, &raw_lines);
        let prepared =
            prepare_overlay_lines(&title, raw_lines, rect.width.saturating_sub(4), rect.height);
        let lines = render_grid(&app, 100, 16);
        let screen = screen_text(&lines);
        let top = line_index(&lines, "┌Output");
        let bottom = line_index(&lines, "└");

        assert_render_invariants(&lines, 100, 16);
        assert_eq!(rect.height, 6, "{screen}");
        assert_eq!(
            prepared,
            vec![
                String::from("demo / agents"),
                String::from("Running | 0s ago"),
                String::from("Summary"),
                String::from("  write tests"),
            ]
        );
        assert_eq!(
            bottom - top + 1,
            usize::from(rect.height),
            "summary-only Output should not carry empty rows:\n{screen}"
        );
        assert!(screen.contains("Summary"), "{screen}");
        assert!(screen.contains("write tests"), "{screen}");
        assert!(!screen.contains("Latest"), "{screen}");
    }

    #[test]
    fn usability_empty_command_center_hugs_recovery_without_blank_chrome() {
        let mut empty = app_with_panes(Vec::new(), vec![]);
        empty.show_command_center();

        let mut no_match = app_with_panes(
            vec![sample_pane("codex")],
            vec![("%1", vec!["Press Enter to continue."])],
        );
        no_match.show_command_center();
        no_match.begin_search();
        for ch in "zzz-no-match".chars() {
            no_match.push_search_char(ch);
        }
        no_match.finish_search();

        for (name, app, expected) in [
            (
                "empty",
                empty,
                vec![
                    String::from("No panes yet."),
                    String::from("Action: start tmux panes, then R refresh"),
                ],
            ),
            (
                "no match",
                no_match,
                vec![
                    String::from("No matching panes."),
                    String::from("Action: backspace show all panes"),
                ],
            ),
        ] {
            let (title, raw_lines) = app
                .overlay_panel()
                .unwrap_or_else(|| panic!("{name} Command Center should be visible"));
            let body = body_rect(100, 16);
            let rect = overlay_rect(body, &title, &raw_lines);
            let prepared =
                prepare_overlay_lines(&title, raw_lines, rect.width.saturating_sub(4), rect.height);
            let lines = render_grid(&app, 100, 16);
            let screen = screen_text(&lines);
            let top = line_index(&lines, "┌Command Center");
            let bottom = line_index(&lines, "└");

            assert_render_invariants(&lines, 100, 16);
            assert_eq!(title, "Command Center");
            assert_eq!(
                rect.height, 4,
                "{name} should not carry empty overlay rows:\n{screen}"
            );
            assert_eq!(prepared, expected, "{name}\n{screen}");
            assert_eq!(
                bottom - top + 1,
                usize::from(rect.height),
                "{name} Command Center should hug state plus action:\n{screen}"
            );
            assert!(lines[top + 1].contains(&prepared[0]), "{name}\n{screen}");
            assert!(lines[top + 2].contains(&prepared[1]), "{name}\n{screen}");
        }
    }

    #[test]
    fn usability_empty_browse_hugs_recovery_without_blank_chrome() {
        let app = app_from_panel_fixture(&panel_fixture("navigator_empty_state"));
        let (title, raw_lines) = app
            .overlay_panel()
            .expect("empty Browse should render as an overlay");
        let body = body_rect(100, 16);
        let rect = overlay_rect(body, &title, &raw_lines);
        let prepared =
            prepare_overlay_lines(&title, raw_lines, rect.width.saturating_sub(4), rect.height);
        let lines = render_grid(&app, 100, 16);
        let screen = screen_text(&lines);
        let top = line_index(&lines, "┌Browse");
        let bottom = line_index(&lines, "└");

        assert_render_invariants(&lines, 100, 16);
        assert_eq!(title, "Browse");
        assert_eq!(
            rect.height, 4,
            "empty Browse should not carry empty overlay rows:\n{screen}"
        );
        assert_eq!(
            prepared,
            vec![
                String::from("No matching panes."),
                String::from("Action: backspace show all panes"),
            ],
            "{screen}"
        );
        assert_eq!(
            bottom - top + 1,
            usize::from(rect.height),
            "empty Browse should hug state plus action:\n{screen}"
        );
        assert!(lines[top + 1].contains(&prepared[0]), "{screen}");
        assert!(lines[top + 2].contains(&prepared[1]), "{screen}");
    }

    fn assert_screen_avoids_retired_user_terms(screen: &str) {
        for term in [
            "Ready to send",
            "staged send",
            "staged command",
            "Enter inspect",
            "Space target",
            "target panes",
            "target set",
            "target group",
            "Target group",
            "Selected pane",
            "inspects here",
            "Smart action",
            "smart action",
            "smart move",
            "G tmux",
            "G jump",
            ". actions",
            "Space send",
            "T send list",
            "Flags:",
            "quick jump",
            "target-set",
            "ack selected attention",
            "clear selected acknowledgement",
            "Send mode for",
            "Search panes. Type",
            "Editing pane search",
            "More open",
            "Help open",
            "actions-menu",
            "Local pane metrics",
            "local metrics",
            "M metrics",
            "metrics: pid",
            "Bell notifications",
            "SSH-safe: bell only",
            "V bell",
            "toggle bell",
            "desktop alerts (ssh-safe)",
            "desktop alerts (terminal)",
            "no desktop notifier found",
            "Desktop alerts on, SSH-safe",
            "poll one-line summaries",
            "Requested one-line summaries",
            "Next:",
            "Next step:",
            "J/K scroll",
        ] {
            assert!(
                !screen.contains(term),
                "retired user-facing term `{term}` appeared in:\n{screen}"
            );
        }
    }

    fn assert_screen_hides_internal_protocol(screen: &str) {
        for term in [
            "STATUS=",
            "BLOCKER=",
            "NEXT=",
            "STATUS=<status>",
            "BLOCKER=<blocker>",
            "NEXT=<next>",
            "Blocked: none",
            "Problem: none",
        ] {
            assert!(
                !screen.contains(term),
                "internal protocol `{term}` leaked into the UI:\n{screen}"
            );
        }
    }

    fn assert_scan_ready_screen(
        name: &str,
        lines: &[String],
        header_term: &str,
        body_terms: &[&str],
        footer_term: &str,
    ) {
        let screen = screen_text(lines);
        assert!(
            lines.first().is_some_and(|line| line.contains("muxboard")),
            "{name} should identify the app immediately:\n{screen}"
        );
        assert!(
            lines.first().is_some_and(|line| line.contains(header_term)),
            "{name} should identify location `{header_term}` in the header:\n{screen}"
        );
        let footer = lines
            .last()
            .unwrap_or_else(|| panic!("{name} rendered no lines"));
        let help_overlay_footer = footer.starts_with("Esc close")
            && lines.first().is_some_and(|line| line.contains("Help"));
        let text_entry_footer = footer.contains("type ") && footer.contains("Esc cancel");
        if help_overlay_footer {
            assert!(
                !screen.contains("? help"),
                "{name} should not advertise opening Help while Help is open:\n{screen}"
            );
        } else if text_entry_footer {
            assert!(
                !footer.contains("? help"),
                "{name} should not advertise ? help while ? is valid typed text:\n{screen}"
            );
        } else {
            assert_eq!(
                screen.matches("? help").count(),
                1,
                "{name} should render exactly one help affordance:\n{screen}"
            );
            assert!(
                footer.contains("? help"),
                "{name} should keep help visible in the footer:\n{screen}"
            );
        }
        assert!(
            footer.contains(footer_term),
            "{name} should expose primary action `{footer_term}` in the footer:\n{screen}"
        );
        for term in body_terms {
            assert!(
                screen.contains(term),
                "{name} should show body signal `{term}`:\n{screen}"
            );
        }
        assert_screen_avoids_retired_user_terms(&screen);
        assert_screen_hides_internal_protocol(&screen);
    }

    fn assert_screen_has_one_line_chrome(name: &str, lines: &[String]) {
        let screen = screen_text(lines);
        let footer = lines
            .last()
            .unwrap_or_else(|| panic!("{name} rendered no lines"));
        let help_overlay_footer = footer.starts_with("Esc close")
            && lines.first().is_some_and(|line| line.contains("Help"));
        let text_entry_footer = footer.contains("type ") && footer.contains("Esc cancel");
        assert!(
            lines.first().is_some_and(|line| line.contains("muxboard")),
            "{name} should keep app identity in the first row:\n{screen}"
        );
        if help_overlay_footer {
            assert!(
                !screen.contains("? help"),
                "{name} should not advertise opening Help while Help is open:\n{screen}"
            );
        } else if text_entry_footer {
            assert!(
                !footer.contains("? help"),
                "{name} should not advertise ? help while ? is valid typed text:\n{screen}"
            );
        } else {
            assert!(
                footer.contains("? help"),
                "{name} should keep help discoverable in the footer:\n{screen}"
            );
            assert_eq!(
                screen.matches("? help").count(),
                1,
                "{name} should not duplicate the help affordance:\n{screen}"
            );
        }
    }

    fn assert_no_low_value_copy(name: &str, lines: &[String]) {
        let screen = screen_text(lines);
        for term in [
            "Updated: no output yet",
            "Latest: no output yet",
            "none / none",
            "send to send list",
            "send to list",
            "pane(s)",
            "group(s)",
            "alert(s)",
            "next nothing waiting",
            "shown next move",
            "Nothing needs attention",
            "action none waiting",
            "attention 0 wait",
            "attention 1 wait",
            "vars {session}",
            "add panes",
            "no recent output",
            "report stat :",
            "report blk  :",
            "report next :",
            "report age  :",
            "Rows: > current",
            "save send group",
            "load saved group",
            "delete saved group",
            "Name this send group",
            "save group",
            "load group",
            "delete group",
            "Saved group",
            "No saved groups",
            "group triage",
            "zoom, groups",
            "load next fleet",
            "choose action",
            " / %",
            "send yes",
            "send no",
        ] {
            assert!(
                !screen.contains(term),
                "{name} should not render low-value copy `{term}`:\n{screen}"
            );
        }
        assert_screen_avoids_retired_user_terms(&screen);
        assert_screen_hides_internal_protocol(&screen);
    }

    fn assert_footer_keeps_core_keys(name: &str, lines: &[String]) {
        let screen = screen_text(lines);
        let footer = lines
            .last()
            .unwrap_or_else(|| panic!("{name} rendered no footer"));
        for term in ["? help", "J/K", ": send", "/ filter", "Q quit"] {
            assert!(
                footer.contains(term),
                "{name} footer should keep `{term}` visible:\n{screen}"
            );
        }
    }

    fn assert_populated_browse_footer_has_only_browse_actions(name: &str, lines: &[String]) {
        let screen = screen_text(lines);
        let footer = lines
            .last()
            .unwrap_or_else(|| panic!("{name} rendered no footer"));
        for term in [
            "? help",
            "J/K browse",
            "Enter window",
            "/ filter",
            "Esc back",
            "Q quit",
        ] {
            assert!(
                footer.contains(term),
                "{name} footer missing Browse action `{term}`:\n{screen}"
            );
        }
        for inert in ["Space add", ": send"] {
            assert!(
                !footer.contains(inert),
                "{name} footer advertised inert action `{inert}`:\n{screen}"
            );
        }
    }

    fn assert_empty_browse_footer_has_only_recovery_actions(name: &str, lines: &[String]) {
        let screen = screen_text(lines);
        let footer = lines
            .last()
            .unwrap_or_else(|| panic!("{name} rendered no footer"));
        for term in [
            "? help",
            "backspace show all",
            "/ filter",
            ". more",
            "Esc back",
            "Q quit",
        ] {
            assert!(
                footer.contains(term),
                "{name} footer missing recovery action `{term}`:\n{screen}"
            );
        }
        for inert in [
            "J/K browse",
            "Enter window",
            "G show",
            "Space add",
            ": send",
        ] {
            assert!(
                !footer.contains(inert),
                "{name} footer advertised inert action `{inert}`:\n{screen}"
            );
        }
    }

    #[test]
    fn no_match_fleet_footer_lists_recovery_not_pane_actions() {
        let app = app_from_view_model_fixture(&view_fixture("empty_search_result_board_title"));
        let lines = render_grid(&app, 100, 18);
        let screen = screen_text(&lines);
        let footer = lines.last().expect("footer should render");

        assert!(screen.contains("No matching panes."), "{screen}");
        assert!(
            screen.contains("Action: backspace show all panes"),
            "{screen}"
        );
        for term in [
            "? help",
            "backspace show all",
            "/ filter",
            ". more",
            "Q quit",
        ] {
            assert!(
                footer.contains(term),
                "empty search footer should expose recovery action `{term}`:\n{screen}"
            );
        }
        for inert in [
            "J/K move",
            "Enter output",
            "G show",
            "Space add",
            ": send",
            "Tab focus",
        ] {
            assert!(
                !footer.contains(inert),
                "empty search footer advertised pane-only action `{inert}`:\n{screen}"
            );
        }

        let marked = app_from_view_model_fixture(&view_fixture("search_mode_compact_board_title"));
        let marked_lines = render_grid(&marked, 68, 18);
        let marked_screen = screen_text(&marked_lines);
        let marked_footer = marked_lines.last().expect("footer should render");

        assert_render_invariants(&marked_lines, 68, 18);
        assert!(
            marked_screen.contains("No matching panes."),
            "{marked_screen}"
        );
        for term in ["1 pane hidden", ": send", "X clear", "backspace show all"] {
            assert!(
                marked_footer.contains(term),
                "hidden send-list footer should expose `{term}`:\n{marked_screen}"
            );
        }
        for inert in ["J/K move", "Space add", "Space remove"] {
            assert!(
                !marked_footer.contains(inert),
                "hidden send-list footer advertised inert action `{inert}`:\n{marked_screen}"
            );
        }
    }

    #[test]
    fn filtered_details_footer_renders_whole_key_hints_at_cell_level() {
        let mut pane = sample_pane("claude");
        pane.id = String::from("%1");
        let output = vec![
            "Claude Code",
            "Dialog open: Allow command? [y/n]",
            "Worker request: run cargo test usability_",
            "line 04",
            "line 05",
            "line 06",
            "line 07",
            "line 08",
            "line 09",
            "line 10",
            "line 11",
            "line 12",
        ];
        let mut app = app_with_panes(vec![pane], vec![("%1", output)]);
        app.set_search_query_for_test("claude");
        app.cycle_panel_focus();

        let lines = render_grid(&app, 120, 36);
        let screen = screen_text(&lines);
        let footer = lines.last().expect("footer should render");

        assert_render_invariants(&lines, 120, 36);
        assert!(footer.contains("backspace show all"), "{screen}");
        assert!(footer.contains("J/K move"), "{screen}");
        assert!(!footer.contains("K older/J newer"), "{screen}");
        assert!(footer.contains(". more"), "{screen}");
        assert!(!footer.contains(". mor..."), "{screen}");
        assert!(!footer.contains("..."), "{screen}");
    }

    fn trunk_text(lines: &[String]) -> String {
        let mut trunk = Vec::new();
        if let Some(header) = lines.first() {
            trunk.push(header.clone());
        }
        trunk.extend(
            lines
                .iter()
                .filter(|line| line.contains('┌'))
                .cloned()
                .collect::<Vec<_>>(),
        );
        if let Some(footer) = lines.last() {
            trunk.push(footer.clone());
        }
        trunk.join("\n")
    }

    #[test]
    fn rendered_primary_journeys_avoid_retired_user_terms() {
        let mut scenarios = vec![
            app_from_panel_fixture(&panel_fixture("selected_waiting_panel")),
            app_from_panel_fixture(&panel_fixture("actions_menu_sections")),
            app_from_panel_fixture(&panel_fixture("send_panel_confirm_dispatch")),
            app_from_view_model_fixture(&view_fixture("command_input_context")),
        ];

        let mut output_app = app_with_panes(
            vec![sample_pane("codex")],
            vec![(
                "%1",
                vec!["STATUS=running | BLOCKER=none | NEXT=write tests"],
            )],
        );
        output_app.cycle_context_pane();
        scenarios.push(output_app);

        for app in scenarios {
            for &(width, height) in &[(80, 16), (120, 24)] {
                let lines = render_grid(&app, width, height);
                assert_render_invariants(&lines, width, height);
                assert_screen_avoids_retired_user_terms(&screen_text(&lines));
            }
        }
    }

    #[test]
    fn usability_trunk_test_preserves_wayfinding_without_body_copy() {
        let mut help = app_from_view_model_fixture(&view_fixture("attention_board_row_summary"));
        help.toggle_help_overlay();

        let scenarios = vec![
            (
                "selected",
                app_from_panel_fixture(&panel_fixture("selected_waiting_panel")),
                vec![
                    "muxboard",
                    "demo/agents",
                    "Fleet",
                    "Details",
                    "Enter output",
                ],
            ),
            (
                "output",
                app_from_panel_fixture(&panel_fixture("live_tail_with_summary_and_raw_tail")),
                vec!["muxboard", "Output", "Esc back"],
            ),
            (
                "more",
                app_from_panel_fixture(&panel_fixture("actions_menu_sections")),
                vec!["muxboard", "More", "Esc close"],
            ),
            (
                "send",
                app_from_view_model_fixture(&view_fixture("command_input_context")),
                vec!["muxboard", "Send to", "Send", "Enter send"],
            ),
            (
                "review",
                app_from_view_model_fixture(&view_fixture("confirm_dispatch_header_and_footer")),
                vec![
                    "muxboard",
                    "Review send",
                    "Send",
                    "Enter send",
                    "Esc cancel",
                ],
            ),
            ("help", help, vec!["muxboard", "Help", "Esc close"]),
        ];

        for (name, app, expected_terms) in scenarios {
            let lines = render_grid(&app, 100, 18);
            let trunk = trunk_text(&lines);
            assert_render_invariants(&lines, 100, 18);
            for term in expected_terms {
                assert!(
                    trunk.contains(term),
                    "{name} trunk should expose `{term}` without reading body copy:\n{trunk}"
                );
            }
        }
    }

    #[test]
    fn usability_critical_journeys_are_scan_ready() {
        let mut help = app_from_view_model_fixture(&view_fixture("attention_board_row_summary"));
        help.toggle_help_overlay();
        let mut output = app_with_panes(
            vec![sample_pane("bash")],
            vec![(
                "%1",
                vec![
                    "download dependencies",
                    "compile crate",
                    "run unit tests",
                    "package binary",
                ],
            )],
        );
        output.cycle_context_pane();
        output.toggle_help_overlay();
        output.close_help_overlay();

        let scenarios = vec![
            (
                "first-load triage",
                app_from_panel_fixture(&panel_fixture("selected_waiting_panel")),
                "demo/agents",
                vec!["Fleet", "Details", "State: Waiting", "Blocked:", "Action:"],
                "Enter output",
            ),
            (
                "inspect output",
                output,
                "Output",
                vec!["Output", "Summary", "Latest"],
                "Esc back",
            ),
            (
                "send command",
                app_from_view_model_fixture(&view_fixture("command_input_context")),
                "Send to",
                vec!["Send", "To:", "Text:", "Preview"],
                "Enter send",
            ),
            (
                "review send",
                app_from_view_model_fixture(&view_fixture("confirm_dispatch_header_and_footer")),
                "Review send",
                vec!["Send", "send to"],
                "Enter send",
            ),
            (
                "more actions",
                app_from_panel_fixture(&panel_fixture("actions_menu_sections")),
                "More",
                vec!["More", "Action:", "send"],
                "Esc close",
            ),
            (
                "browse",
                app_from_panel_fixture(&panel_fixture("navigator_empty_state")),
                "Browse",
                vec![
                    "Browse",
                    "No matching panes.",
                    "Action: backspace show all panes",
                ],
                "backspace show all",
            ),
            (
                "help",
                help,
                "Help",
                vec!["Help", "Now:", "Move:"],
                "Esc close",
            ),
        ];

        for (name, app, header_term, body_terms, footer_term) in scenarios {
            for &(width, height) in &[(100, 18), (120, 22)] {
                let lines = render_grid(&app, width, height);
                assert_render_invariants(&lines, width, height);
                assert_scan_ready_screen(name, &lines, header_term, &body_terms, footer_term);
            }
        }
    }

    #[test]
    fn usability_scripted_journeys_stay_obvious_step_by_step() {
        let mut first = sample_pane("codex");
        first.id = String::from("%1");
        let mut second = sample_pane("claude");
        second.id = String::from("%2");
        second.active = false;
        second.pane_index = 1;
        let mut app = app_with_panes(
            vec![first, second],
            vec![
                (
                    "%1",
                    vec!["STATUS=waiting | BLOCKER=approval | NEXT=approve deploy"],
                ),
                (
                    "%2",
                    vec!["STATUS=running | BLOCKER=none | NEXT=write tests"],
                ),
            ],
        );
        let runtime = tokio::runtime::Runtime::new().expect("runtime should build");

        let assert_step = |app: &App, name: &str, terms: &[&str]| {
            let lines = render_grid(app, 100, 18);
            let screen = screen_text(&lines);
            assert_render_invariants(&lines, 100, 18);
            assert_screen_has_one_line_chrome(name, &lines);
            assert_no_low_value_copy(name, &lines);
            for term in terms {
                assert!(screen.contains(term), "{name} missing `{term}`:\n{screen}");
            }
        };

        assert_step(
            &app,
            "first load",
            &["Fleet", "Details", "Action: : reply", "Enter output"],
        );

        app.cycle_context_pane();
        assert_step(&app, "open output", &["Output", "Summary", "approval"]);
        app.cycle_context_pane();
        app.cycle_context_pane();
        app.cycle_context_pane();
        app.cycle_context_pane();
        assert_step(&app, "return to details", &["Details", "State: Waiting"]);

        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Char('/')))
            .expect("search should open");
        for ch in "zznomatch".chars() {
            runtime
                .block_on(handle_key_press(&mut app, KeyCode::Char(ch)))
                .expect("search typing should work");
        }
        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Enter))
            .expect("search should apply");
        assert_step(
            &app,
            "empty search",
            &[
                "no matches",
                "No matching panes.",
                "Action: backspace show all panes",
            ],
        );

        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Char('/')))
            .expect("search should reopen");
        for _ in 0.."zznomatch".chars().count() {
            runtime
                .block_on(handle_key_press(&mut app, KeyCode::Backspace))
                .expect("search backspace should work");
        }
        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Enter))
            .expect("search clear should apply");
        assert_step(
            &app,
            "search recovery",
            &["Fleet", "Details", "Action: : reply"],
        );

        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Char(' ')))
            .expect("mark first pane should work");
        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Char('j')))
            .expect("move should work");
        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Char(' ')))
            .expect("mark second pane should work");
        assert_step(&app, "send list", &["send list 2 panes", ": send"]);

        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Char(':')))
            .expect("send should open");
        for ch in "echo hello".chars() {
            runtime
                .block_on(handle_key_press(&mut app, KeyCode::Char(ch)))
                .expect("command typing should work");
        }
        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Enter))
            .expect("send should stage for multiple panes");
        assert_step(
            &app,
            "review send",
            &[
                "Review send to the send list (2 panes)",
                "Enter send",
                "Esc cancel",
            ],
        );
        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Esc))
            .expect("review cancel should work");
        assert_step(&app, "review recovery", &["send list 2 panes", "? help"]);
    }

    #[test]
    fn usability_action_contract_footer_keys_execute_visible_actions() {
        let mut first = sample_pane("codex");
        first.id = String::from("%1");
        let mut second = sample_pane("claude");
        second.id = String::from("%2");
        second.window_id = String::from("@2");
        second.window_name = String::from("review");
        second.active = false;
        second.pane_index = 1;
        let mut app = app_with_panes(
            vec![first, second],
            vec![
                (
                    "%1",
                    vec![
                        "STATUS=waiting | BLOCKER=approval | NEXT=approve deploy",
                        "step 01 read prompt",
                        "step 02 prepare fix",
                        "step 03 run tests",
                        "step 04 summarize",
                    ],
                ),
                (
                    "%2",
                    vec!["STATUS=running | BLOCKER=none | NEXT=write tests"],
                ),
            ],
        );
        let runtime = tokio::runtime::Runtime::new().expect("runtime should build");

        let assert_screen = |app: &App, label: &str, body: &[&str], footer_terms: &[&str]| {
            let lines = render_grid(app, 110, 20);
            let screen = screen_text(&lines);
            let footer = lines
                .last()
                .unwrap_or_else(|| panic!("{label} rendered no footer"));
            assert_render_invariants(&lines, 110, 20);
            assert_screen_has_one_line_chrome(label, &lines);
            assert_no_low_value_copy(label, &lines);
            for term in body {
                assert!(
                    screen.contains(term),
                    "{label} missing body term `{term}`:\n{screen}"
                );
            }
            for term in footer_terms {
                assert!(
                    footer.contains(term),
                    "{label} footer missing promised action `{term}`:\n{screen}"
                );
            }
        };

        let press = |app: &mut App, key| {
            runtime
                .block_on(handle_key_press(app, key))
                .expect("visible key action should run")
        };
        let type_text = |app: &mut App, text: &str| {
            for ch in text.chars() {
                runtime
                    .block_on(handle_key_press(app, KeyCode::Char(ch)))
                    .expect("visible text input should run");
            }
        };

        assert_screen(
            &app,
            "initial footer contract",
            &["Fleet", "Details", "Action: : reply"],
            &["Enter output", "Space add", ": reply", "/ filter", ". more"],
        );

        press(&mut app, KeyCode::Char('/'));
        assert_screen(
            &app,
            "search opens from footer",
            &["Search panes."],
            &["Enter apply", "Esc cancel"],
        );
        type_text(&mut app, "zznomatch");
        press(&mut app, KeyCode::Enter);
        assert_screen(
            &app,
            "search enter applies",
            &[
                "no matches",
                "No matching panes.",
                "Action: backspace show all panes",
            ],
            &["backspace show all", "/ filter"],
        );
        press(&mut app, KeyCode::Backspace);
        assert_screen(
            &app,
            "backspace recovers search",
            &["Fleet", "Details", "Action: : reply"],
            &["Enter output", ": reply"],
        );

        press(&mut app, KeyCode::Char(' '));
        assert_screen(
            &app,
            "space adds send-list pane",
            &["send list 1 pane"],
            &["send list 1 pane", "Space remove", ": send"],
        );
        press(&mut app, KeyCode::Char('j'));
        press(&mut app, KeyCode::Char(' '));
        assert_screen(
            &app,
            "movement and space grow send list",
            &["send list 2 panes"],
            &["send list 2 panes", "Space remove", ": send"],
        );

        press(&mut app, KeyCode::Char(':'));
        assert_screen(
            &app,
            "colon opens send",
            &["Send", "send list (2 panes)"],
            &["Enter review", "Esc cancel"],
        );
        type_text(&mut app, "echo hello");
        press(&mut app, KeyCode::Enter);
        assert_screen(
            &app,
            "enter stages multi-send review",
            &["Review send to the send list (2 panes)", "Text: echo hello"],
            &["Enter send", "Esc cancel"],
        );
        press(&mut app, KeyCode::Esc);
        assert_screen(
            &app,
            "escape cancels review without losing send list",
            &["send list 2 panes"],
            &["send list 2 panes", ": send"],
        );

        press(&mut app, KeyCode::Char('.'));
        assert_screen(
            &app,
            "dot opens more",
            &["More", "browse windows", "command center"],
            &["press a listed key", "Esc close"],
        );
        press(&mut app, KeyCode::Char('['));
        assert_screen(
            &app,
            "browse action opens browse",
            &["Browse"],
            &["J/K browse", "Enter window", "Esc back"],
        );
        press(&mut app, KeyCode::Esc);
        assert_screen(
            &app,
            "escape backs out of browse",
            &["Details"],
            &["Esc back", ": send"],
        );
        press(&mut app, KeyCode::Esc);
        assert_screen(
            &app,
            "escape returns details focus to fleet",
            &["Fleet", "Details"],
            &["J/K move", ": send"],
        );

        press(&mut app, KeyCode::Enter);
        assert_screen(&app, "enter opens output", &["Output"], &["Esc back"]);
        let output_screen = render_grid(&app, 110, 20);
        press(&mut app, KeyCode::Enter);
        assert_eq!(
            normalize_relative_ages(render_grid(&app, 110, 20)),
            normalize_relative_ages(output_screen),
            "Enter must not secretly act as Back inside Output"
        );
        press(&mut app, KeyCode::Esc);
        assert_screen(
            &app,
            "escape backs output to details",
            &["Details"],
            &[": send"],
        );
    }

    #[test]
    fn usability_action_contract_reply_line_keys_execute_visible_actions() {
        let runtime = tokio::runtime::Runtime::new().expect("runtime should build");
        let noop_tmux = fake_tmux_script("reply-line-actions", "#!/bin/sh\nexit 0\n");

        let mut continue_app = app_with_panes(
            vec![sample_pane("codex")],
            vec![("%1", vec!["Press Enter to continue."])],
        );
        use_fake_tmux_for_test(&mut continue_app, noop_tmux.clone());
        let screen = screen_text(&render_grid(&continue_app, 110, 20));
        assert!(screen.contains("Action: A continue"), "{screen}");
        assert!(screen.contains("Also: : send"), "{screen}");
        runtime
            .block_on(handle_key_press(&mut continue_app, KeyCode::Char('a')))
            .expect("reply continue key should execute");
        assert_eq!(
            continue_app.status_message(),
            "Sent Enter to demo / agents."
        );

        let mut answer_app = app_with_panes(
            vec![sample_pane("claude")],
            vec![("%1", vec!["Allow command? [y/n]"])],
        );
        use_fake_tmux_for_test(&mut answer_app, noop_tmux);
        let screen = screen_text(&render_grid(&answer_app, 110, 20));
        assert!(screen.contains("Action: . answer yes/no"), "{screen}");
        assert!(screen.contains("Also: : send, G show"), "{screen}");
        runtime
            .block_on(handle_key_press(&mut answer_app, KeyCode::Char('.')))
            .expect("reply answer key should open answer options");
        let answer_menu = screen_text(&render_grid(&answer_app, 110, 20));
        assert!(answer_menu.contains("Y answer yes"), "{answer_menu}");
        assert!(answer_menu.contains("N answer no"), "{answer_menu}");
        runtime
            .block_on(handle_key_press(&mut answer_app, KeyCode::Char('y')))
            .expect("listed answer key should send yes");
        assert_eq!(
            answer_app.status_message(),
            "Sent y + Enter to demo / agents."
        );

        let mut answer_app = app_with_panes(
            vec![sample_pane("claude")],
            vec![("%1", vec!["Allow command? [y/n]"])],
        );
        use_fake_tmux_for_test(
            &mut answer_app,
            fake_tmux_script("reply-line-show", "#!/bin/sh\nexit 0\n"),
        );
        runtime
            .block_on(handle_key_press(&mut answer_app, KeyCode::Char(':')))
            .expect("reply type-response key should open Send");
        assert!(answer_app.is_command_input_active());
        runtime
            .block_on(handle_key_press(&mut answer_app, KeyCode::Esc))
            .expect("escape should leave Send before testing show");
        runtime
            .block_on(handle_key_press(&mut answer_app, KeyCode::Char('g')))
            .expect("reply show key should execute");
        assert_eq!(
            answer_app.status_message(),
            "Showing demo / agents in tmux. Muxboard is still running."
        );
    }

    #[test]
    fn usability_action_contract_reply_line_non_choice_keys_execute_visible_actions() {
        let runtime = tokio::runtime::Runtime::new().expect("runtime should build");
        let noop_tmux = fake_tmux_script("reply-line-actions-non-choice", "#!/bin/sh\nexit 0\n");

        let mut answer_app = app_with_panes(
            vec![sample_pane("claude")],
            vec![("%1", vec!["approval needed"])],
        );
        use_fake_tmux_for_test(&mut answer_app, noop_tmux);
        let screen = screen_text(&render_grid(&answer_app, 110, 20));
        assert!(screen.contains("Action: : reply"), "{screen}");
        assert!(!screen.contains("Reply:"), "{screen}");
        runtime
            .block_on(handle_key_press(&mut answer_app, KeyCode::Char(':')))
            .expect("reply type-response key should open Reply");
        assert!(answer_app.is_command_input_active());
        let reply_screen = screen_text(&render_grid(&answer_app, 110, 20));
        assert!(reply_screen.contains("Reply"), "{reply_screen}");
        assert!(
            reply_screen.contains("Reply to: demo / agents"),
            "{reply_screen}"
        );
        assert!(reply_screen.contains("Enter reply"), "{reply_screen}");
        assert!(!reply_screen.contains("Enter send"), "{reply_screen}");
        assert!(!reply_screen.contains("Recent"), "{reply_screen}");
        runtime
            .block_on(handle_key_press(&mut answer_app, KeyCode::Esc))
            .expect("escape should leave Reply before testing show");
        runtime
            .block_on(handle_key_press(&mut answer_app, KeyCode::Char('g')))
            .expect("reply show key should execute");
        assert_eq!(
            answer_app.status_message(),
            "Showing demo / agents in tmux. Muxboard is still running."
        );
    }

    #[test]
    fn usability_action_contract_help_refresh_executes_visible_action() {
        let fake_tmux = fake_tmux_script(
            "top-level-refresh",
            "#!/bin/sh\n\
if [ \"$1\" = \"-V\" ]; then echo 'tmux fake'; exit 0; fi\n\
if [ \"$1\" = \"list-panes\" ]; then printf '$1\\tdemo\\t@1\\tagents\\t%%1\\t0\\t123\\tcodex\\tcodex\\t/tmp\\t1\\t0\\n'; exit 0; fi\n\
if [ \"$1\" = \"capture-pane\" ]; then echo 'STATUS=running | BLOCKER=none | NEXT=keep testing'; exit 0; fi\n\
exit 0\n",
        );
        let mut app = app_with_panes(
            vec![sample_pane("codex")],
            vec![(
                "%1",
                vec!["STATUS=waiting | BLOCKER=approval | NEXT=approve"],
            )],
        );
        use_fake_tmux_for_test(&mut app, fake_tmux);

        let runtime = tokio::runtime::Runtime::new().expect("runtime should build");
        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Char('?')))
            .expect("help should open");
        let help_lines = render_grid(&app, 110, 20);
        let help = screen_text(&help_lines);
        assert_render_invariants(&help_lines, 110, 20);
        assert!(help.contains("Help"), "{help}");
        assert!(help.contains("R refresh"), "{help}");
        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Esc))
            .expect("help should close");

        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Char('r')))
            .expect("visible help refresh action should run");

        let refreshed = screen_text(&render_grid(&app, 110, 20));
        assert_eq!(app.status_message(), "Refreshed.");
        assert!(refreshed.contains("keep testing"), "{refreshed}");
        assert!(!app.should_quit());
    }

    #[test]
    fn usability_action_contract_start_agent_visible_keys_launch_window() {
        let log_path = std::env::temp_dir().join(format!(
            "muxboard-start-agent-{}-{}.log",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock should be after unix epoch")
                .as_nanos()
        ));
        let fake_tmux = fake_tmux_script(
            "start-agent-visible-keys",
            &format!(
                "#!/bin/sh\n\
if [ \"$1\" = \"-V\" ]; then echo 'tmux fake'; exit 0; fi\n\
if [ \"$1\" = \"new-window\" ]; then printf '%s\\n' \"$*\" >> '{}'; exit 0; fi\n\
if [ \"$1\" = \"list-panes\" ]; then printf '$1\\tdemo\\t@1\\tagents\\t%%1\\t0\\t123\\tcodex\\tcodex\\t/workspace\\t1\\t0\\n$1\\tdemo\\t@2\\tbash\\t%%2\\t0\\t124\\tbash\\tbash\\t/workspace\\t0\\t0\\n'; exit 0; fi\n\
if [ \"$1\" = \"capture-pane\" ]; then echo 'ready'; exit 0; fi\n\
exit 0\n",
                log_path.display()
            ),
        );
        let mut app = app_with_panes(vec![sample_pane("codex")], vec![("%1", vec!["ready"])]);
        use_fake_tmux_for_test(&mut app, fake_tmux);

        let runtime = tokio::runtime::Runtime::new().expect("runtime should build");
        let press = |app: &mut App, key| {
            runtime
                .block_on(handle_key_press(app, key))
                .expect("Start visible key should route")
        };
        press(&mut app, KeyCode::Char('.'));
        press(&mut app, KeyCode::Char('+'));

        let start_lines = render_grid(&app, 100, 20);
        let start = screen_text(&start_lines);
        assert_render_invariants(&start_lines, 100, 20);
        assert!(app.is_launch_input_active(), "{start}");
        assert!(start.contains("Start agent."), "{start}");
        assert!(start.contains("In: demo / agents"), "{start}");
        assert!(start.contains("codex"), "{start}");
        for footer_term in ["Tab preset", "Enter start", "Esc cancel"] {
            assert!(start.contains(footer_term), "{start}");
        }

        press(&mut app, KeyCode::Char('x'));
        press(&mut app, KeyCode::Backspace);
        press(&mut app, KeyCode::Left);
        assert!(app.is_launch_input_active());

        press(&mut app, KeyCode::BackTab);
        let preset = screen_text(&render_grid(&app, 100, 20));
        assert!(preset.contains("bash"), "{preset}");

        press(&mut app, KeyCode::Enter);

        assert!(!app.is_launch_input_active());
        assert_eq!(app.status_message(), "Started `bash` in demo/bash.");
        let log = std::fs::read_to_string(&log_path).expect("fake tmux log should be written");
        assert!(
            log.contains("new-window -d -t demo -n bash -c /workspace bash"),
            "Start should launch the visible command in the selected pane folder:\n{log}"
        );
        let refreshed = screen_text(&render_grid(&app, 100, 20));
        assert!(refreshed.contains("demo/bash"), "{refreshed}");
        let _ = std::fs::remove_file(log_path);
    }

    #[test]
    fn usability_action_contract_more_menu_actions_execute_visible_promises() {
        let fake_tmux = fake_tmux_script(
            "tui-actions",
            "#!/bin/sh\nif [ \"$1\" = \"-V\" ]; then echo 'tmux fake'; exit 0; fi\nif [ \"$1\" = \"display-message\" ]; then echo '/dev/ttys999'; exit 0; fi\nexit 0\n",
        );
        let build_app = || {
            let mut waiting = sample_pane("codex");
            waiting.id = String::from("%1");
            let mut running = sample_pane("claude");
            running.id = String::from("%2");
            running.window_id = String::from("@2");
            running.window_name = String::from("review");
            running.pane_index = 1;
            running.active = false;
            let mut app = app_with_panes(
                vec![waiting, running],
                vec![
                    (
                        "%1",
                        vec![
                            "Waiting for approval. Continue?",
                            "Press Enter to continue",
                            "Proceed? (y/n)",
                        ],
                    ),
                    ("%2", vec!["running tests"]),
                ],
            );
            use_fake_tmux_for_test(&mut app, fake_tmux.clone());
            app.toggle_selected_mark();
            app
        };
        let build_acknowledged_app = || {
            let mut waiting = sample_pane("codex");
            waiting.id = String::from("%1");
            let mut app =
                app_with_panes(vec![waiting], vec![("%1", vec!["Waiting for approval."])]);
            use_fake_tmux_for_test(&mut app, fake_tmux.clone());
            app.acknowledge_selected_attention();
            app
        };
        let build_jump_app = || {
            let mut app = app_with_panes(vec![sample_pane("codex")], vec![]);
            use_fake_tmux_for_test(&mut app, fake_tmux.clone());
            app
        };
        let mut base = build_app();
        base.open_action_menu();
        let menu_lines = render_grid(&base, 130, 36);
        let menu = screen_text(&menu_lines);
        assert_render_invariants(&menu_lines, 130, 36);
        for promised in [
            "Enter show output",
            ": send text",
            "[ browse windows",
            "] command center",
            "S summarize panes",
            "R refresh",
            "+ start agent",
            "C mute alert",
            "Z zoom pane",
            "Y answer yes",
            "N answer no",
            "X clear send list",
            "G save fleet",
            "L layout: auto",
        ] {
            assert!(
                menu.contains(promised),
                "More menu should visibly promise `{promised}`:\n{menu}"
            );
        }
        let mut acknowledged = build_acknowledged_app();
        acknowledged.open_action_menu();
        let acknowledged_menu_lines = render_grid(&acknowledged, 130, 36);
        let acknowledged_menu = screen_text(&acknowledged_menu_lines);
        assert_render_invariants(&acknowledged_menu_lines, 130, 36);
        assert!(
            acknowledged_menu.contains("W unmute alert"),
            "More menu should visibly promise selected alert recovery after muting:\n{acknowledged_menu}"
        );
        assert!(
            acknowledged
                .command_lines()
                .iter()
                .any(|line| line.contains("U unmute all")),
            "More command model should expose fleet-wide alert recovery after muting"
        );
        let runtime = tokio::runtime::Runtime::new().expect("runtime should build");
        let mut jump_menu_app = build_jump_app();
        jump_menu_app.open_action_menu();
        let jump_menu = screen_text(&render_grid(&jump_menu_app, 130, 36));
        assert!(
            jump_menu.contains("G show in tmux"),
            "More menu should visibly promise attach/show when no same-key save action is active:\n{jump_menu}"
        );
        let mut reply_app = app_with_panes(
            vec![sample_pane("codex")],
            vec![("%1", vec!["Type your answer to continue."])],
        );
        use_fake_tmux_for_test(&mut reply_app, fake_tmux.clone());
        reply_app.open_action_menu();
        let reply_menu = screen_text(&render_grid(&reply_app, 130, 36));
        assert!(reply_menu.contains("Action: : reply"), "{reply_menu}");
        runtime
            .block_on(handle_key_press(&mut reply_app, KeyCode::Char(':')))
            .expect("More reply action should route");
        assert!(!reply_app.is_action_menu_active(), "{reply_menu}");
        assert!(
            reply_app.is_command_input_active(),
            "More promised `: reply` but did not open the text composer:\n{reply_menu}"
        );

        let exercise = |key: KeyCode, expected: &[&str], mut app: App| {
            app.open_action_menu();
            runtime
                .block_on(handle_key_press(&mut app, key))
                .unwrap_or_else(|error| panic!("More action {key:?} should execute: {error}"));
            assert!(
                !app.is_action_menu_active(),
                "More action {key:?} should leave the menu"
            );
            let screen = screen_text(&render_grid(&app, 120, 24));
            for term in expected {
                assert!(
                    screen.contains(term) || app.status_message().contains(term),
                    "More action {key:?} missing `{term}`:\nstatus: {}\n{screen}",
                    app.status_message()
                );
            }
        };

        exercise(KeyCode::Enter, &["Output", "Showing output"], build_app());
        exercise(KeyCode::Char(':'), &["Send"], build_app());
        exercise(KeyCode::Char('.'), &["Details"], build_app());
        exercise(KeyCode::Char('['), &["Browse"], build_app());
        exercise(KeyCode::Char(']'), &["Command Center"], build_app());
        exercise(KeyCode::Char('+'), &["Start"], build_app());
        exercise(
            KeyCode::Char('s'),
            &["Asked 1 pane for a one-line summary"],
            build_app(),
        );
        exercise(KeyCode::Char('r'), &["Refreshed."], build_app());
        exercise(KeyCode::Char('t'), &["Sorted by"], build_app());
        exercise(KeyCode::Char('f'), &["Showing agents"], build_app());
        exercise(KeyCode::Char('i'), &["Sent Enter to"], build_app());
        exercise(KeyCode::Char('c'), &["Muted alert"], build_app());
        exercise(
            KeyCode::Char('w'),
            &["Unmuted alert"],
            build_acknowledged_app(),
        );
        exercise(
            KeyCode::Char('g'),
            &["Showing demo / agents"],
            build_jump_app(),
        );
        exercise(KeyCode::Char('z'), &["Toggled zoom"], build_app());
        exercise(KeyCode::Char('e'), &["Sent Enter"], build_app());
        exercise(KeyCode::Char('y'), &["Sent y + Enter"], build_app());
        exercise(KeyCode::Char('n'), &["Sent n + Enter"], build_app());
        exercise(KeyCode::Char('b'), &["Lane send enabled"], build_app());
        exercise(KeyCode::Char('a'), &["Muted"], build_app());
        exercise(
            KeyCode::Char('u'),
            &["Unmuted 1 alert"],
            build_acknowledged_app(),
        );
        exercise(KeyCode::Char('x'), &["Cleared 1"], build_app());
        exercise(KeyCode::Char('g'), &["Name this fleet"], build_app());
        exercise(KeyCode::Char('L'), &["Layout: side by side"], build_app());
        exercise(KeyCode::Char('m'), &["Pane CPU/memory shown"], build_app());
        exercise(KeyCode::Char('o'), &["Desktop alerts"], build_app());
        exercise(KeyCode::Char('v'), &["Terminal bell"], build_app());
        exercise(KeyCode::Char('h'), &["Alert repeat delay"], build_app());
        exercise(KeyCode::Char('p'), &["Alerts:"], build_app());
    }

    #[test]
    fn usability_action_contract_fleet_picker_footer_keys_execute_visible_actions() {
        let mut first = sample_pane("codex");
        first.id = String::from("%1");
        first.window_name = String::from("alpha");

        let mut second = sample_pane("claude");
        second.id = String::from("%2");
        second.window_id = String::from("@2");
        second.window_name = String::from("beta");
        second.active = false;
        second.pane_index = 1;

        let mut app = app_with_panes(vec![first, second], vec![]);
        app.set_target_groups_for_test(vec![
            crate::app::TargetGroup {
                name: String::from("triage"),
                members: vec![crate::app::PaneLocator {
                    session_name: String::from("demo"),
                    window_name: String::from("alpha"),
                    pane_index: 0,
                }],
            },
            crate::app::TargetGroup {
                name: String::from("review"),
                members: vec![crate::app::PaneLocator {
                    session_name: String::from("demo"),
                    window_name: String::from("beta"),
                    pane_index: 1,
                }],
            },
        ]);
        app.open_fleet_picker();

        let runtime = tokio::runtime::Runtime::new().expect("runtime should build");
        let press = |app: &mut App, key| {
            runtime
                .block_on(handle_key_press(app, key))
                .expect("visible fleet picker key should execute")
        };
        let assert_picker = |app: &App, label: &str, expected_row: &str| {
            let lines = render_grid(app, 96, 18);
            let screen = screen_text(&lines);
            let footer = lines.last().expect("footer should render");
            assert_render_invariants(&lines, 96, 18);
            assert!(screen.contains("Fleets"), "{label}\n{screen}");
            assert!(screen.contains(expected_row), "{label}\n{screen}");
            for term in ["J/K choose", "Enter load", "D delete", "Esc close"] {
                assert!(
                    footer.contains(term),
                    "{label} footer should advertise `{term}`:\n{screen}"
                );
            }
            assert_no_low_value_copy(label, &lines);
        };

        assert_picker(&app, "initial picker", "> triage");

        press(&mut app, KeyCode::Char('?'));
        let help = screen_text(&render_grid(&app, 96, 18));
        assert!(help.contains("Help"), "{help}");
        assert!(help.contains("Esc close"), "{help}");
        press(&mut app, KeyCode::Esc);
        assert!(app.is_fleet_picker_active());
        assert!(!app.is_help_overlay_active());

        press(&mut app, KeyCode::Char('j'));
        assert_picker(&app, "move down", "> review");
        press(&mut app, KeyCode::Char('k'));
        assert_picker(&app, "move up", "> triage");

        press(&mut app, KeyCode::Char('l'));
        assert!(!app.is_fleet_picker_active());
        assert_eq!(
            app.status_message(),
            "Loaded fleet `triage` with 1 pane live."
        );

        app.open_fleet_picker();
        press(&mut app, KeyCode::Char('d'));
        assert!(app.is_fleet_picker_active());
        assert_eq!(app.status_message(), "Deleted fleet `triage`.");
        assert_picker(&app, "delete keeps picker recoverable", "> review");

        press(&mut app, KeyCode::Esc);
        assert!(!app.is_fleet_picker_active());
        assert_eq!(app.status_message(), "Closed Fleets.");
    }

    #[test]
    fn usability_action_contract_save_fleet_visible_keys_persist_named_fleet() {
        let mut first = sample_pane("codex");
        first.id = String::from("%1");
        first.window_name = String::from("alpha");

        let mut second = sample_pane("claude");
        second.id = String::from("%2");
        second.window_id = String::from("@2");
        second.window_name = String::from("beta");
        second.active = false;
        second.pane_index = 1;

        let mut app = app_with_panes(vec![first, second], vec![("%1", vec!["ready"])]);
        app.toggle_selected_mark();

        let runtime = tokio::runtime::Runtime::new().expect("runtime should build");
        let press = |app: &mut App, key| {
            runtime
                .block_on(handle_key_press(app, key))
                .expect("visible save fleet key should execute")
        };

        press(&mut app, KeyCode::Char('.'));
        let more = screen_text(&render_grid(&app, 120, 24));
        assert!(more.contains("G save fleet"), "{more}");

        press(&mut app, KeyCode::Char('g'));
        let naming_lines = render_grid(&app, 104, 20);
        let naming = screen_text(&naming_lines);
        let footer = naming_lines.last().expect("footer should render");
        assert_render_invariants(&naming_lines, 104, 20);
        assert!(app.is_group_input_active(), "{naming}");
        assert!(
            naming.contains("Save this send list as a reusable fleet."),
            "{naming}"
        );
        assert!(naming.contains("Name:"), "{naming}");
        for footer_term in ["type name", "Enter save", "Esc cancel"] {
            assert!(footer.contains(footer_term), "{naming}");
        }

        for ch in ['t', 'r', 'i', 'a', 'g', 'x'] {
            press(&mut app, KeyCode::Char(ch));
        }
        press(&mut app, KeyCode::Backspace);
        press(&mut app, KeyCode::Char('e'));
        press(&mut app, KeyCode::Left);
        let typed = screen_text(&render_grid(&app, 104, 20));
        assert!(typed.contains("Name: triage"), "{typed}");
        assert!(app.is_group_input_active(), "{typed}");

        press(&mut app, KeyCode::Enter);
        assert!(!app.is_group_input_active());
        assert_eq!(app.status_message(), "Saved fleet `triage` with 1 pane.");

        app.open_fleet_picker();
        let picker = screen_text(&render_grid(&app, 104, 20));
        assert!(picker.contains("Fleets"), "{picker}");
        assert!(picker.contains("> triage"), "{picker}");
        assert!(picker.contains("1/1 live current"), "{picker}");
    }

    #[test]
    fn usability_action_contract_more_saved_fleet_rows_execute_visible_actions() {
        let mut first = sample_pane("codex");
        first.id = String::from("%1");
        first.window_name = String::from("alpha");

        let mut second = sample_pane("claude");
        second.id = String::from("%2");
        second.window_id = String::from("@2");
        second.window_name = String::from("beta");
        second.active = false;
        second.pane_index = 1;

        let mut app = app_with_panes(vec![first, second], vec![]);
        app.set_target_groups_for_test(vec![
            crate::app::TargetGroup {
                name: String::from("triage"),
                members: vec![crate::app::PaneLocator {
                    session_name: String::from("demo"),
                    window_name: String::from("alpha"),
                    pane_index: 0,
                }],
            },
            crate::app::TargetGroup {
                name: String::from("review"),
                members: vec![crate::app::PaneLocator {
                    session_name: String::from("demo"),
                    window_name: String::from("beta"),
                    pane_index: 1,
                }],
            },
        ]);
        app.load_next_target_group();
        app.open_action_menu();

        let runtime = tokio::runtime::Runtime::new().expect("runtime should build");
        let press = |app: &mut App, key| {
            runtime
                .block_on(handle_key_press(app, key))
                .expect("visible More key should execute")
        };

        let menu_lines = render_grid(&app, 120, 28);
        let menu = screen_text(&menu_lines);
        assert_render_invariants(&menu_lines, 120, 28);
        for promised in ["L choose fleet", "D delete triage"] {
            assert!(
                menu.contains(promised),
                "More should visibly promise saved-fleet action `{promised}`:\n{menu}"
            );
        }

        press(&mut app, KeyCode::Char('l'));
        assert!(app.is_fleet_picker_active(), "{menu}");
        assert!(!app.is_action_menu_active(), "{menu}");
        press(&mut app, KeyCode::Esc);

        app.open_action_menu();
        press(&mut app, KeyCode::Char('d'));
        assert!(!app.is_action_menu_active());
        assert_eq!(app.status_message(), "Deleted fleet `triage`.");
        let remaining = app.fleet_picker_lines();
        assert_eq!(remaining, vec![String::from("> review  1/1 live")]);

        let mut pane = sample_pane("codex");
        pane.id = String::from("%1");
        pane.window_name = String::from("alpha");
        let mut stale = app_with_panes(vec![pane], vec![]);
        stale.set_target_groups_for_test(vec![crate::app::TargetGroup {
            name: String::from("triage"),
            members: vec![crate::app::PaneLocator {
                session_name: String::from("demo"),
                window_name: String::from("missing"),
                pane_index: 0,
            }],
        }]);
        stale.load_next_target_group();
        stale.open_action_menu();
        let stale_menu = screen_text(&render_grid(&stale, 96, 20));
        assert!(stale_menu.contains("L choose fleet"), "{stale_menu}");
        assert!(stale_menu.contains("D delete stale triage"), "{stale_menu}");
        assert!(
            !stale_menu.contains(": send text"),
            "stale More must not advertise sending to a dead fleet:\n{stale_menu}"
        );

        press(&mut stale, KeyCode::Char('l'));
        assert!(stale.is_fleet_picker_active(), "{stale_menu}");
        assert!(!stale.is_action_menu_active(), "{stale_menu}");
        press(&mut stale, KeyCode::Esc);

        stale.open_action_menu();
        press(&mut stale, KeyCode::Char('d'));
        assert!(!stale.is_action_menu_active());
        assert_eq!(stale.status_message(), "Deleted fleet `triage`.");
        assert_eq!(
            stale.fleet_picker_lines(),
            vec![
                String::from("No saved fleets."),
                String::from("Mark panes, then save a fleet from More.")
            ]
        );
    }

    #[test]
    fn usability_action_contract_more_enter_goes_back_from_output_to_details() {
        let mut app = app_with_panes(
            vec![sample_pane("codex")],
            vec![(
                "%1",
                vec![
                    "STATUS=running | BLOCKER=none | NEXT=write tests",
                    "running unit tests",
                ],
            )],
        );
        app.cycle_context_pane();
        assert!(app.is_output_view_active());
        app.open_action_menu();

        let menu_lines = render_grid(&app, 100, 20);
        let menu = screen_text(&menu_lines);
        assert_render_invariants(&menu_lines, 100, 20);
        assert!(
            menu.contains("Enter show details"),
            "More should describe Enter as returning to Details from Output:\n{menu}"
        );

        let runtime = tokio::runtime::Runtime::new().expect("runtime should build");
        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Enter))
            .expect("More Enter should route");

        let screen = screen_text(&render_grid(&app, 100, 20));
        assert!(!app.is_action_menu_active(), "{screen}");
        assert!(!app.is_output_view_active(), "{screen}");
        assert!(screen.contains("Details"), "{screen}");
    }

    #[test]
    fn usability_action_contract_rebound_keys_drive_actions_not_just_copy() {
        let fake_tmux = fake_tmux_script(
            "tui-rebound-actions",
            "#!/bin/sh\nif [ \"$1\" = \"-V\" ]; then echo 'tmux fake'; exit 0; fi\nif [ \"$1\" = \"display-message\" ]; then echo '/dev/ttys999'; exit 0; fi\nexit 0\n",
        );
        let build_app = || {
            let mut first = sample_pane("codex");
            first.id = String::from("%1");
            let mut second = sample_pane("claude");
            second.id = String::from("%2");
            second.window_id = String::from("@2");
            second.window_name = String::from("review");
            second.pane_index = 1;
            second.active = false;
            let mut app = app_with_panes(
                vec![first, second],
                vec![
                    ("%1", vec!["Press Enter to continue"]),
                    ("%2", vec!["running tests"]),
                ],
            );
            use_fake_tmux_for_test(&mut app, fake_tmux.clone());
            apply_rebound_keybindings(&mut app);
            app
        };
        let runtime = tokio::runtime::Runtime::new().expect("runtime should build");
        let press = |app: &mut App, key| {
            runtime
                .block_on(handle_key_press(app, key))
                .expect("key action should route")
        };

        let mut stale = build_app();
        for key in [
            KeyCode::Enter,
            KeyCode::Char('.'),
            KeyCode::Char('/'),
            KeyCode::Char(':'),
            KeyCode::Char(' '),
        ] {
            press(&mut stale, key);
        }
        let stale_screen = screen_text(&render_grid(&stale, 120, 22));
        assert!(!stale.is_output_view_active(), "{stale_screen}");
        assert!(!stale.is_action_menu_active(), "{stale_screen}");
        assert!(!stale.is_search_input_active(), "{stale_screen}");
        assert!(!stale.is_command_input_active(), "{stale_screen}");
        assert!(
            !stale_screen.contains("send list"),
            "stale Space should not mark a target after Space is rebound:\n{stale_screen}"
        );

        let mut app = build_app();
        let footer = render_grid(&app, 140, 22)
            .last()
            .expect("footer should render")
            .to_owned();
        for term in [
            "N/P move", "O output", "L show", "V add", "C send", "F filter", "M more", "Z quit",
            "8 layout",
        ] {
            assert!(footer.contains(term), "footer missing `{term}`:\n{footer}");
        }

        press(&mut app, KeyCode::Char('v'));
        let marked_screen = screen_text(&render_grid(&app, 120, 22));
        assert!(
            marked_screen.contains("send list 1 pane"),
            "V should mark the selected target:\n{marked_screen}"
        );

        press(&mut app, KeyCode::Char('c'));
        assert!(app.is_command_input_active(), "{marked_screen}");
        press(&mut app, KeyCode::Esc);
        press(&mut app, KeyCode::Char('f'));
        assert!(app.is_search_input_active());
        press(&mut app, KeyCode::Esc);

        press(&mut app, KeyCode::Char('o'));
        assert!(app.is_output_view_active());
        press(&mut app, KeyCode::Esc);

        press(&mut app, KeyCode::Char('m'));
        assert!(app.is_action_menu_active());
        let more = screen_text(&render_grid(&app, 120, 28));
        for term in ["; continue waiting", "0 zoom pane"] {
            assert!(
                more.contains(term),
                "More missing rebound `{term}`:\n{more}"
            );
        }
        press(&mut app, KeyCode::Char('0'));
        assert!(!app.is_action_menu_active());
        assert!(app.status_message().contains("Toggled zoom"));

        press(&mut app, KeyCode::Char('m'));
        press(&mut app, KeyCode::Char(';'));
        assert!(!app.is_action_menu_active());
        assert!(app.status_message().contains("Sent Enter to"));

        press(&mut app, KeyCode::Char('z'));
        assert!(app.should_quit());
    }

    #[test]
    fn usability_action_contract_help_promises_execute_visible_actions() {
        let build_app = || {
            app_with_panes(
                vec![sample_pane("codex")],
                vec![("%1", vec!["Press Enter to continue"])],
            )
        };
        let runtime = tokio::runtime::Runtime::new().expect("runtime should build");
        let press = |app: &mut App, key| {
            runtime
                .block_on(handle_key_press(app, key))
                .expect("help action should route")
        };

        let mut app = build_app();
        press(&mut app, KeyCode::Char('?'));
        let help_lines = render_grid(&app, 96, 18);
        let help = screen_text(&help_lines);
        assert_render_invariants(&help_lines, 96, 18);
        assert!(app.is_help_overlay_active());
        assert!(help.contains("Help"), "{help}");
        assert!(
            help.contains("Close: Esc backs out or closes Help, Q quit muxboard."),
            "{help}"
        );
        assert!(help.contains("L layout"), "{help}");
        assert!(help.contains(". then [ browse"), "{help}");
        assert!(help.contains("] command center"), "{help}");

        let mut enter_from_help = build_app();
        press(&mut enter_from_help, KeyCode::Char('?'));
        press(&mut enter_from_help, KeyCode::Enter);
        assert!(!enter_from_help.is_help_overlay_active());
        assert!(
            enter_from_help.is_output_view_active(),
            "Help promised Enter output, so Enter must work from Help"
        );

        press(&mut app, KeyCode::Char('?'));
        assert!(!app.is_help_overlay_active());
        assert!(!app.should_quit());

        press(&mut app, KeyCode::Char('?'));
        press(&mut app, KeyCode::Char('L'));
        assert!(!app.is_help_overlay_active());
        assert_eq!(app.layout_preset(), LayoutPreset::Horizontal);
        assert_eq!(app.status_message(), "Layout: side by side.");

        let mut browse_from_help = build_app();
        press(&mut browse_from_help, KeyCode::Char('?'));
        press(&mut browse_from_help, KeyCode::Char('.'));
        assert!(browse_from_help.is_action_menu_active());
        press(&mut browse_from_help, KeyCode::Char('['));
        assert!(!browse_from_help.is_help_overlay_active());
        assert!(!browse_from_help.is_action_menu_active());
        assert_eq!(browse_from_help.context_panel_title(), "Browse");

        let mut command_center_from_help = build_app();
        press(&mut command_center_from_help, KeyCode::Char('?'));
        press(&mut command_center_from_help, KeyCode::Char('.'));
        assert!(command_center_from_help.is_action_menu_active());
        press(&mut command_center_from_help, KeyCode::Char(']'));
        assert!(!command_center_from_help.is_help_overlay_active());
        assert!(!command_center_from_help.is_action_menu_active());
        assert_eq!(
            command_center_from_help.context_panel_title(),
            "Command Center"
        );

        press(&mut app, KeyCode::Char('?'));
        press(&mut app, KeyCode::Char('q'));
        assert!(
            app.should_quit(),
            "Help promised Q quit, so Q must not be swallowed while Help is open"
        );

        let mut output = build_app();
        press(&mut output, KeyCode::Enter);
        assert!(output.is_output_view_active());
        press(&mut output, KeyCode::Char('?'));
        assert!(output.is_help_overlay_active());
        press(&mut output, KeyCode::Esc);
        assert!(!output.is_help_overlay_active());
        assert!(
            output.is_output_view_active(),
            "Esc should close Help before backing out of Output"
        );
        press(&mut output, KeyCode::Esc);
        assert!(!output.is_output_view_active());

        let mut choice = app_with_panes(
            vec![sample_pane("claude")],
            vec![("%1", vec!["Allow command? [y/n]"])],
        );
        use_fake_tmux_for_test(
            &mut choice,
            fake_tmux_script("help-choice-answer", "#!/bin/sh\nexit 0\n"),
        );
        press(&mut choice, KeyCode::Char('?'));
        let choice_help = screen_text(&render_grid(&choice, 110, 20));
        assert!(
            choice_help.contains("Now: . answer yes/no, : send, G show in tmux."),
            "{choice_help}"
        );
        press(&mut choice, KeyCode::Char('.'));
        assert!(!choice.is_help_overlay_active());
        assert!(choice.is_action_menu_active(), "{choice_help}");
        press(&mut choice, KeyCode::Char('y'));
        assert_eq!(choice.status_message(), "Sent y + Enter to demo / agents.");

        let mut prompt = app_with_panes(
            vec![sample_pane("codex")],
            vec![("%1", vec!["Type your answer to continue."])],
        );
        press(&mut prompt, KeyCode::Char('?'));
        let prompt_help = screen_text(&render_grid(&prompt, 110, 20));
        assert!(
            prompt_help.contains("Now: : reply, Enter output, G show in tmux."),
            "{prompt_help}"
        );
        press(&mut prompt, KeyCode::Char(':'));
        assert!(!prompt.is_help_overlay_active());
        assert!(prompt.is_command_input_active(), "{prompt_help}");

        let mut rebound = build_app();
        apply_rebound_keybindings(&mut rebound);
        press(&mut rebound, KeyCode::Char('?'));
        let rebound_help = screen_text(&render_grid(&rebound, 120, 20));
        assert!(
            rebound_help.contains("Close: Esc backs out or closes Help, Z quit muxboard."),
            "{rebound_help}"
        );
        press(&mut rebound, KeyCode::Char('q'));
        assert!(
            !rebound.should_quit(),
            "old default Q must be inert after quit is rebound:\n{rebound_help}"
        );
        press(&mut rebound, KeyCode::Char('z'));
        assert!(
            rebound.should_quit(),
            "rebound quit key should work while Help is open"
        );
    }

    #[test]
    fn usability_action_contract_empty_help_recovery_actions_are_real() {
        let runtime = tokio::runtime::Runtime::new().expect("runtime should build");
        let press = |app: &mut App, key| {
            runtime
                .block_on(handle_key_press(app, key))
                .expect("empty Help action should route")
        };

        let mut empty = app_with_panes(Vec::new(), vec![]);
        press(&mut empty, KeyCode::Char('?'));
        let empty_help = screen_text(&render_grid(&empty, 96, 18));
        assert!(
            empty_help.contains("More: . layout and settings."),
            "{empty_help}"
        );
        press(&mut empty, KeyCode::Char('.'));
        assert!(!empty.is_help_overlay_active(), "{empty_help}");
        assert!(empty.is_action_menu_active(), "{empty_help}");
        let empty_more = screen_text(&render_grid(&empty, 96, 18));
        assert!(empty_more.contains("More"), "{empty_more}");
        assert!(
            empty_more.contains("Action: R refresh after starting tmux panes"),
            "{empty_more}"
        );

        let mut no_match = app_with_panes(vec![sample_pane("codex")], vec![]);
        no_match.set_search_query_for_test("zz-no-match");
        press(&mut no_match, KeyCode::Char('?'));
        let no_match_help = screen_text(&render_grid(&no_match, 96, 18));
        assert!(
            no_match_help.contains("Now: backspace show all panes."),
            "{no_match_help}"
        );
        press(&mut no_match, KeyCode::Backspace);
        assert!(!no_match.is_help_overlay_active(), "{no_match_help}");
        let recovered = screen_text(&render_grid(&no_match, 96, 18));
        assert!(recovered.contains("demo/agents"), "{recovered}");
        assert!(!recovered.contains("No matching panes."), "{recovered}");
    }

    #[test]
    fn usability_action_contract_more_help_is_real_and_recoverable() {
        let mut app = app_with_panes(
            vec![sample_pane("codex")],
            vec![("%1", vec!["Press Enter to continue"])],
        );
        let runtime = tokio::runtime::Runtime::new().expect("runtime should build");
        let press = |app: &mut App, key| {
            runtime
                .block_on(handle_key_press(app, key))
                .expect("More help action should route")
        };

        press(&mut app, KeyCode::Char('.'));
        let more = screen_text(&render_grid(&app, 100, 20));
        assert!(app.is_action_menu_active(), "{more}");
        assert!(more.contains("More"), "{more}");
        assert!(
            more.lines()
                .last()
                .is_some_and(|line| line.contains("? help")),
            "More advertised help in footer:\n{more}"
        );

        press(&mut app, KeyCode::Char('?'));
        let help_lines = render_grid(&app, 100, 20);
        let help = screen_text(&help_lines);
        assert_render_invariants(&help_lines, 100, 20);
        assert!(app.is_action_menu_active());
        assert!(app.is_help_overlay_active());
        assert!(help.contains("Help"), "{help}");
        assert!(
            help.contains("Now: press a listed key, Esc closes More."),
            "{help}"
        );

        press(&mut app, KeyCode::Esc);
        let restored_more = screen_text(&render_grid(&app, 100, 20));
        assert!(!app.is_help_overlay_active());
        assert!(app.is_action_menu_active(), "{restored_more}");
        assert!(restored_more.contains("More"), "{restored_more}");

        press(&mut app, KeyCode::Esc);
        assert!(!app.is_action_menu_active());

        press(&mut app, KeyCode::Char('.'));
        press(&mut app, KeyCode::Char('?'));
        press(&mut app, KeyCode::Char('q'));
        assert!(
            app.should_quit(),
            "Q should still quit from Help layered over More"
        );
    }

    #[test]
    fn usability_action_contract_non_text_modal_help_is_real_and_layered() {
        let runtime = tokio::runtime::Runtime::new().expect("runtime should build");
        let press = |app: &mut App, key| {
            runtime
                .block_on(handle_key_press(app, key))
                .expect("modal help action should route")
        };

        let mut review =
            app_from_view_model_fixture(&view_fixture("confirm_dispatch_header_and_footer"));
        assert!(review.has_pending_dispatch());
        assert!(review.footer_line_for_width(100).contains("? help"));
        press(&mut review, KeyCode::Char('?'));
        let review_help = screen_text(&render_grid(&review, 100, 20));
        assert!(review.is_help_overlay_active());
        assert!(review.has_pending_dispatch());
        assert!(
            review_help.contains("Now: Enter sends, Esc cancels review."),
            "{review_help}"
        );
        press(&mut review, KeyCode::Esc);
        assert!(!review.is_help_overlay_active());
        assert!(review.has_pending_dispatch());
        press(&mut review, KeyCode::Esc);
        assert!(!review.has_pending_dispatch());

        let mut macro_assign =
            app_with_panes(vec![sample_pane("codex")], vec![("%1", vec!["ready"])]);
        remember_command_for_test(&mut macro_assign, "cargo test");
        macro_assign.show_send_view();
        press(&mut macro_assign, KeyCode::Char('p'));
        assert!(macro_assign.is_macro_assign_active());
        assert!(macro_assign.footer_line_for_width(100).contains("? help"));
        press(&mut macro_assign, KeyCode::Char('?'));
        let macro_help = screen_text(&render_grid(&macro_assign, 100, 20));
        assert!(macro_assign.is_help_overlay_active());
        assert!(macro_assign.is_macro_assign_active());
        assert!(
            macro_help.contains("pins latest command, Esc cancels."),
            "{macro_help}"
        );
        press(&mut macro_assign, KeyCode::Esc);
        assert!(!macro_assign.is_help_overlay_active());
        assert!(macro_assign.is_macro_assign_active());
        press(&mut macro_assign, KeyCode::Esc);
        assert!(!macro_assign.is_macro_assign_active());

        let mut fleet = app_with_panes(vec![sample_pane("codex")], vec![("%1", vec!["ready"])]);
        fleet.set_target_groups_for_test(vec![crate::app::TargetGroup {
            name: String::from("triage"),
            members: vec![crate::app::PaneLocator {
                session_name: String::from("demo"),
                window_name: String::from("agents"),
                pane_index: 0,
            }],
        }]);
        fleet.open_fleet_picker();
        assert!(fleet.is_fleet_picker_active());
        assert!(fleet.footer_line_for_width(100).contains("? help"));
        press(&mut fleet, KeyCode::Char('?'));
        let fleet_help = screen_text(&render_grid(&fleet, 100, 20));
        assert!(fleet.is_help_overlay_active());
        assert!(fleet.is_fleet_picker_active());
        assert!(
            fleet_help.contains("choose fleet, Enter loads, Esc closes fleets."),
            "{fleet_help}"
        );
        press(&mut fleet, KeyCode::Esc);
        assert!(!fleet.is_help_overlay_active());
        assert!(fleet.is_fleet_picker_active());
        press(&mut fleet, KeyCode::Esc);
        assert!(!fleet.is_fleet_picker_active());
    }

    #[test]
    fn usability_action_contract_review_send_enter_executes_visible_confirmation() {
        let log_path = std::env::temp_dir().join(format!(
            "muxboard-review-send-{}-{}.log",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock should be after unix epoch")
                .as_nanos()
        ));
        let fake_tmux = fake_tmux_script(
            "review-send-enter",
            &format!(
                "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nexit 0\n",
                log_path.display()
            ),
        );
        let mut app =
            app_from_view_model_fixture(&view_fixture("confirm_dispatch_header_and_footer"));
        use_fake_tmux_for_test(&mut app, fake_tmux);

        let lines = render_grid(&app, 100, 20);
        let screen = screen_text(&lines);
        assert_render_invariants(&lines, 100, 20);
        assert!(
            screen.contains("Review send to fleet triage (2 panes)."),
            "{screen}"
        );
        assert!(screen.contains("Enter send"), "{screen}");
        assert!(screen.contains("Esc cancel"), "{screen}");

        let runtime = tokio::runtime::Runtime::new().expect("runtime should build");
        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Char('j')))
            .expect("unlisted review key should be ignored safely");
        assert!(app.has_pending_dispatch());

        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Enter))
            .expect("visible review Enter action should send");

        assert!(!app.has_pending_dispatch());
        assert_eq!(
            app.status_message(),
            "Sent `continue` to fleet triage (2 panes)."
        );
        let sent = std::fs::read_to_string(&log_path).expect("fake tmux log should be written");
        for expected in [
            "send-keys -t %1 -l -- continue",
            "send-keys -t %1 Enter",
            "send-keys -t %2 -l -- continue",
            "send-keys -t %2 Enter",
        ] {
            assert!(
                sent.contains(expected),
                "review send should dispatch `{expected}`:\n{sent}"
            );
        }
        let _ = std::fs::remove_file(log_path);
    }

    #[test]
    fn usability_action_contract_send_enter_dispatches_single_visible_target() {
        let log_path = std::env::temp_dir().join(format!(
            "muxboard-single-send-{}-{}.log",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock should be after unix epoch")
                .as_nanos()
        ));
        let fake_tmux = fake_tmux_script(
            "single-send-enter",
            &format!(
                "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nexit 0\n",
                log_path.display().to_string().replace('\'', "'\\''")
            ),
        );
        let mut app = app_with_panes(vec![sample_pane("codex")], vec![("%1", vec!["ready"])]);
        use_fake_tmux_for_test(&mut app, fake_tmux);
        let runtime = tokio::runtime::Runtime::new().expect("runtime should build");

        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Char(':')))
            .expect("Send should open");
        for ch in "echo hi".chars() {
            runtime
                .block_on(handle_key_press(&mut app, KeyCode::Char(ch)))
                .expect("command typing should stay in Send");
        }

        let lines = render_grid(&app, 100, 20);
        let screen = screen_text(&lines);
        let footer = lines.last().expect("footer should render");
        assert_render_invariants(&lines, 100, 20);
        assert!(app.is_command_input_active(), "{screen}");
        assert!(screen.contains("Send"), "{screen}");
        assert!(screen.contains("echo hi"), "{screen}");
        assert!(footer.contains("Enter send"), "{screen}");
        assert!(footer.contains("Esc cancel"), "{screen}");

        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Enter))
            .expect("visible Send Enter action should dispatch");

        assert!(!app.is_command_input_active());
        assert!(!app.has_pending_dispatch());
        assert_eq!(
            app.status_message(),
            "Sent command `echo hi` to 1 pane in demo / agents."
        );
        assert!(!app.should_quit());
        let sent = std::fs::read_to_string(&log_path).expect("fake tmux log should be written");
        for expected in ["send-keys -t %1 -l -- echo hi", "send-keys -t %1 Enter"] {
            assert!(
                sent.contains(expected),
                "single-target Send Enter should dispatch `{expected}`:\n{sent}"
            );
        }
        let _ = std::fs::remove_file(log_path);

        let after = screen_text(&render_grid(&app, 100, 20));
        assert!(after.contains("Sent command `echo hi`"), "{after}");
        assert!(after.contains("Details"), "{after}");
    }

    #[test]
    fn usability_action_contract_text_modes_do_not_advertise_help_for_question_mark() {
        let mut app = app_with_panes(vec![sample_pane("codex")], vec![("%1", vec!["ready"])]);
        let runtime = tokio::runtime::Runtime::new().expect("runtime should build");
        let press = |app: &mut App, key| {
            runtime
                .block_on(handle_key_press(app, key))
                .expect("text mode action should route")
        };
        let assert_no_help_promise = |app: &App, label: &str| {
            for width in [96, 104, 120] {
                let footer = render_grid(app, width, 20)
                    .last()
                    .unwrap_or_else(|| panic!("{label} rendered no footer"))
                    .to_owned();
                assert!(
                    !footer.contains("? help"),
                    "{label} should not advertise ? help while ? is valid text at {width} cols:\n{footer}"
                );
                assert!(
                    footer.contains("Esc cancel"),
                    "{label} should keep recovery visible at {width} cols:\n{footer}"
                );
            }
        };

        press(&mut app, KeyCode::Char('/'));
        app.set_status_message_for_test("Search updated.");
        assert_no_help_promise(&app, "search input");
        press(&mut app, KeyCode::Char('?'));
        assert!(!app.is_help_overlay_active());
        assert!(app.is_search_input_active());
        press(&mut app, KeyCode::Esc);

        press(&mut app, KeyCode::Char(':'));
        app.set_status_message_for_test("Send text is empty.");
        assert_no_help_promise(&app, "command input");
        press(&mut app, KeyCode::Char('?'));
        assert!(!app.is_help_overlay_active());
        assert!(app.is_command_input_active());
        press(&mut app, KeyCode::Esc);

        app.open_action_menu();
        press(&mut app, KeyCode::Char('+'));
        app.set_status_message_for_test("Start failed: new-window refused");
        assert_no_help_promise(&app, "launch input");
        press(&mut app, KeyCode::Char('?'));
        assert!(!app.is_help_overlay_active());
        assert!(app.is_launch_input_active());
        press(&mut app, KeyCode::Esc);

        app.toggle_selected_mark();
        app.begin_group_save_input();
        app.set_status_message_for_test("Fleet name is empty.");
        assert_no_help_promise(&app, "group input");
        press(&mut app, KeyCode::Char('?'));
        assert!(!app.is_help_overlay_active());
        assert!(app.is_group_input_active());
    }

    #[test]
    fn usability_action_contract_search_and_send_inputs_keep_text_visible_after_stray_keys() {
        let mut app = app_with_panes(vec![sample_pane("codex")], vec![("%1", vec!["ready"])]);
        let runtime = tokio::runtime::Runtime::new().expect("runtime should build");
        let press = |app: &mut App, key| {
            runtime
                .block_on(handle_key_press(app, key))
                .expect("visible text input key should route")
        };
        let type_text = |app: &mut App, text: &str| {
            for ch in text.chars() {
                press(app, KeyCode::Char(ch));
            }
        };
        let assert_footer_terms = |app: &App, label: &str, terms: &[&str]| {
            let lines = render_grid(app, 104, 20);
            let screen = screen_text(&lines);
            let footer = lines
                .last()
                .unwrap_or_else(|| panic!("{label} rendered no footer"));
            assert_render_invariants(&lines, 104, 20);
            for term in terms {
                assert!(
                    footer.contains(term),
                    "{label} footer missing `{term}`:\n{screen}"
                );
            }
        };

        press(&mut app, KeyCode::Char('/'));
        assert_footer_terms(
            &app,
            "search input footer",
            &["type to filter", "Enter apply", "Esc cancel"],
        );
        type_text(&mut app, "codex");
        press(&mut app, KeyCode::Left);
        let search = screen_text(&render_grid(&app, 104, 20));
        assert!(app.is_search_input_active(), "{search}");
        assert!(search.contains("Searching for `codex`."), "{search}");
        press(&mut app, KeyCode::Backspace);
        let shortened_search = screen_text(&render_grid(&app, 104, 20));
        assert!(
            shortened_search.contains("Searching for `code`."),
            "{shortened_search}"
        );
        press(&mut app, KeyCode::Esc);
        assert!(!app.is_search_input_active());

        press(&mut app, KeyCode::Char(':'));
        assert_footer_terms(
            &app,
            "send input footer",
            &["type text", "Enter send", "Esc cancel"],
        );
        type_text(&mut app, "cargo test");
        press(&mut app, KeyCode::Left);
        let send = screen_text(&render_grid(&app, 104, 20));
        assert!(app.is_command_input_active(), "{send}");
        assert!(send.contains("cargo test"), "{send}");
        press(&mut app, KeyCode::Backspace);
        let shortened_send = screen_text(&render_grid(&app, 104, 20));
        assert!(shortened_send.contains("cargo tes"), "{shortened_send}");
        press(&mut app, KeyCode::Esc);
        assert!(!app.is_command_input_active());
    }

    #[test]
    fn usability_action_contract_unlisted_keys_do_not_steal_modal_state() {
        let mut first = sample_pane("codex");
        first.id = String::from("%1");
        first.window_name = String::from("alpha");

        let mut second = sample_pane("claude");
        second.id = String::from("%2");
        second.window_id = String::from("@2");
        second.window_name = String::from("beta");
        second.active = false;
        second.pane_index = 1;

        let mut app = app_with_panes(vec![first, second], vec![]);
        let runtime = tokio::runtime::Runtime::new().expect("runtime should build");
        let press = |app: &mut App, key| {
            runtime
                .block_on(handle_key_press(app, key))
                .expect("visible key should route")
        };
        let assert_selected_row = |app: &App, label: &str, location: &str| {
            let lines = render_grid(app, 104, 20);
            let screen = screen_text(&lines);
            assert_render_invariants(&lines, 104, 20);
            assert!(
                lines
                    .iter()
                    .any(|line| line.contains('>') && line.contains(location)),
                "{label} should visibly select `{location}`:\n{screen}"
            );
        };

        assert_selected_row(&app, "initial selection", "demo/alpha");
        press(&mut app, KeyCode::Char('k'));
        assert_selected_row(&app, "top-level K moves up", "demo/beta");

        press(&mut app, KeyCode::Char('.'));
        let more_before = render_grid(&app, 104, 20);
        for key in [
            KeyCode::Left,
            KeyCode::Backspace,
            KeyCode::Char('c'),
            KeyCode::Char('w'),
            KeyCode::Char('a'),
            KeyCode::Char('u'),
            KeyCode::Char('i'),
            KeyCode::Char('x'),
            KeyCode::Char('y'),
            KeyCode::Char('n'),
        ] {
            press(&mut app, key);
            assert!(app.is_action_menu_active());
            assert_eq!(
                render_grid(&app, 104, 20),
                more_before,
                "unlisted key {key:?} must not mutate the More menu"
            );
        }
        press(&mut app, KeyCode::Esc);
        assert!(!app.is_action_menu_active());

        let mut empty_more = app_with_panes(Vec::new(), vec![]);
        press(&mut empty_more, KeyCode::Char('.'));
        let empty_more_before = render_grid(&empty_more, 104, 20);
        for key in [KeyCode::Char(':'), KeyCode::Char('s')] {
            press(&mut empty_more, key);
            assert!(empty_more.is_action_menu_active());
            assert_eq!(
                render_grid(&empty_more, 104, 20),
                empty_more_before,
                "unlisted empty More key {key:?} must not mutate or close the menu"
            );
        }

        let mut no_match_more = app_with_panes(vec![sample_pane("codex")], vec![]);
        no_match_more.set_search_query_for_test("zz-no-match");
        press(&mut no_match_more, KeyCode::Char('.'));
        let no_match_more_before = render_grid(&no_match_more, 104, 20);
        for key in [KeyCode::Char(':'), KeyCode::Char('s')] {
            press(&mut no_match_more, key);
            assert!(no_match_more.is_action_menu_active());
            assert_eq!(
                render_grid(&no_match_more, 104, 20),
                no_match_more_before,
                "unlisted no-match More key {key:?} must not mutate or close the menu"
            );
        }

        let mut no_match_ack = app_with_panes(
            vec![sample_pane("codex")],
            vec![("%1", vec!["Waiting for approval."])],
        );
        no_match_ack.acknowledge_selected_attention();
        no_match_ack.set_search_query_for_test("zz-no-match");
        press(&mut no_match_ack, KeyCode::Char('.'));
        let no_match_ack_before = render_grid(&no_match_ack, 104, 20);
        for key in [KeyCode::Char('w'), KeyCode::Char('u')] {
            press(&mut no_match_ack, key);
            assert!(no_match_ack.is_action_menu_active());
            assert_eq!(
                render_grid(&no_match_ack, 104, 20),
                no_match_ack_before,
                "hidden acknowledgement key {key:?} must not mutate a no-match More menu"
            );
        }

        let mut narrowed_first = sample_pane("codex");
        narrowed_first.id = String::from("%1");
        narrowed_first.window_name = String::from("alpha");
        let mut narrowed_second = sample_pane("claude");
        narrowed_second.id = String::from("%2");
        narrowed_second.window_id = String::from("@2");
        narrowed_second.window_name = String::from("beta");
        narrowed_second.active = false;
        narrowed_second.pane_index = 1;
        let mut narrowed_more = app_with_panes(vec![narrowed_first, narrowed_second], vec![]);
        narrowed_more.set_search_query_for_test("codex");
        narrowed_more.open_action_menu();
        let narrowed_more_before = render_grid(&narrowed_more, 104, 20);
        let narrowed_more_before_screen = screen_text(&narrowed_more_before);
        assert_render_invariants(&narrowed_more_before, 104, 20);
        assert_eq!(
            narrowed_more
                .command_lines()
                .iter()
                .filter(|line| line.as_str() == "  backspace show all panes")
                .count(),
            1,
            "{narrowed_more_before_screen}"
        );
        assert!(
            !narrowed_more
                .command_lines()
                .iter()
                .any(|line| line.as_str() == "backspace show all panes"),
            "{narrowed_more_before_screen}"
        );
        press(&mut narrowed_more, KeyCode::Backspace);
        let narrowed_more_after = render_grid(&narrowed_more, 104, 20);
        let narrowed_more_after_screen = screen_text(&narrowed_more_after);
        assert_render_invariants(&narrowed_more_after, 104, 20);
        assert!(!narrowed_more.is_action_menu_active());
        let narrowed_more_after_footer = narrowed_more_after
            .last()
            .expect("recovered board footer should render");
        assert!(
            !narrowed_more_after_screen.contains("Showing all panes."),
            "recovery should spend footer space on useful actions, not status-only feedback:\n{narrowed_more_after_screen}"
        );
        for term in ["? help", "J/K move", "Enter output", ": send", ". more"] {
            assert!(
                narrowed_more_after_footer.contains(term),
                "recovered board footer should keep `{term}` visible:\n{narrowed_more_after_screen}"
            );
        }
        assert!(
            !narrowed_more_after_footer.contains("backspace show all"),
            "recovered board footer should not advertise stale narrowing:\n{narrowed_more_after_screen}"
        );
        assert!(
            narrowed_more_after_screen.contains("demo/alpha")
                && narrowed_more_after_screen.contains("demo/beta"),
            "{narrowed_more_after_screen}"
        );

        let mut no_match = app_with_panes(vec![sample_pane("codex")], vec![]);
        no_match.set_search_query_for_test("zz-no-match");
        press(&mut no_match, KeyCode::Char(':'));
        let no_match_lines = render_grid(&no_match, 104, 20);
        let no_match_screen = screen_text(&no_match_lines);
        let no_match_footer = no_match_lines
            .last()
            .expect("no-match screen should have footer");
        assert_render_invariants(&no_match_lines, 104, 20);
        assert!(!no_match.is_command_input_active());
        assert!(
            no_match_screen.contains("Show all panes before sending."),
            "{no_match_screen}"
        );
        assert!(
            no_match_footer.contains("backspace show all"),
            "{no_match_screen}"
        );
        assert!(
            !no_match_screen.contains("Command:"),
            "hidden no-match command key should not open Send:\n{no_match_screen}"
        );

        let mut filtered_empty = app_with_panes(vec![sample_pane("codex")], vec![]);
        filtered_empty.cycle_filter_mode();
        filtered_empty.cycle_filter_mode();
        press(&mut filtered_empty, KeyCode::Char(':'));
        let filtered_lines = render_grid(&filtered_empty, 104, 20);
        let filtered_screen = screen_text(&filtered_lines);
        let filtered_footer = filtered_lines
            .last()
            .expect("filtered-empty screen should have footer");
        assert_render_invariants(&filtered_lines, 104, 20);
        assert!(!filtered_empty.is_command_input_active());
        assert!(
            filtered_screen.contains("Show all panes before sending."),
            "{filtered_screen}"
        );
        assert!(
            filtered_footer.contains("backspace show all"),
            "{filtered_screen}"
        );
        assert!(
            !filtered_screen.contains("Command:"),
            "filtered-empty command key should not open Send:\n{filtered_screen}"
        );

        let mut empty_browse = app_from_panel_fixture(&panel_fixture("navigator_empty_state"));
        let empty_browse_lines = render_grid(&empty_browse, 104, 20);
        let empty_browse_screen = screen_text(&empty_browse_lines);
        assert_render_invariants(&empty_browse_lines, 104, 20);
        assert_empty_browse_footer_has_only_recovery_actions("empty Browse", &empty_browse_lines);
        for key in [KeyCode::Enter, KeyCode::Char('g')] {
            press(&mut empty_browse, key);
        }
        let empty_browse_after_lines = render_grid(&empty_browse, 104, 20);
        let empty_browse_after_screen = screen_text(&empty_browse_after_lines);
        assert_render_invariants(&empty_browse_after_lines, 104, 20);
        assert!(
            empty_browse.is_browse_view_active(),
            "{empty_browse_after_screen}"
        );
        assert!(
            empty_browse_after_screen.contains("No window selected in Browse."),
            "{empty_browse_after_screen}"
        );
        assert_empty_browse_footer_has_only_recovery_actions(
            "empty Browse after inert keys",
            &empty_browse_after_lines,
        );
        assert!(
            !empty_browse_after_screen.contains("Output"),
            "empty Browse Enter/G must not open Output:\n{empty_browse_after_screen}"
        );
        assert!(
            empty_browse_screen.contains("No matching panes."),
            "{empty_browse_screen}"
        );

        let mut more_recovery = app_with_panes(vec![sample_pane("codex")], vec![]);
        more_recovery.set_search_query_for_test("zz-no-match");
        more_recovery.open_action_menu();
        let more_recovery_before = render_grid(&more_recovery, 104, 20);
        let more_recovery_before_screen = screen_text(&more_recovery_before);
        assert_render_invariants(&more_recovery_before, 104, 20);
        assert!(
            more_recovery_before_screen.contains("backspace show all panes"),
            "{more_recovery_before_screen}"
        );
        assert_eq!(
            more_recovery
                .command_lines()
                .iter()
                .filter(|line| line.as_str() == "  backspace show all panes")
                .count(),
            1,
            "{more_recovery_before_screen}"
        );
        assert!(
            !more_recovery
                .command_lines()
                .iter()
                .any(|line| line.as_str() == "backspace show all panes"),
            "{more_recovery_before_screen}"
        );
        press(&mut more_recovery, KeyCode::Backspace);
        let more_recovery_after = render_grid(&more_recovery, 104, 20);
        let more_recovery_after_screen = screen_text(&more_recovery_after);
        assert_render_invariants(&more_recovery_after, 104, 20);
        assert!(!more_recovery.is_action_menu_active());
        let more_recovery_after_footer = more_recovery_after
            .last()
            .expect("more recovery footer should render");
        assert!(
            !more_recovery_after_screen.contains("Showing all panes."),
            "More recovery should spend footer space on useful actions, not status-only feedback:\n{more_recovery_after_screen}"
        );
        for term in ["? help", "J/K move", "Enter output", ": send", ". more"] {
            assert!(
                more_recovery_after_footer.contains(term),
                "More recovery footer should keep `{term}` visible:\n{more_recovery_after_screen}"
            );
        }
        assert!(
            !more_recovery_after_footer.contains("backspace show all"),
            "More recovery footer should not advertise stale narrowing:\n{more_recovery_after_screen}"
        );
        assert!(
            !more_recovery_after_screen.contains("No matching panes."),
            "{more_recovery_after_screen}"
        );

        let log_path = std::env::temp_dir().join(format!(
            "muxboard-hidden-pane-actions-{}-{}.log",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock should be after unix epoch")
                .as_nanos()
        ));
        let fake_tmux = fake_tmux_script(
            "hidden-pane-actions",
            &format!(
                "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nexit 0\n",
                log_path.display().to_string().replace('\'', "'\\''")
            ),
        );
        let hidden_app = || {
            let mut app = app_with_panes(
                vec![sample_pane("codex")],
                vec![("%1", vec!["Press Enter to continue"])],
            );
            use_fake_tmux_for_test(&mut app, fake_tmux.clone());
            app.toggle_selected_mark();
            app.set_search_query_for_test("zz-no-match");
            app
        };

        let mut hidden_top_level = hidden_app();
        let hidden_initial_lines = render_grid(&hidden_top_level, 104, 20);
        let hidden_initial_screen = screen_text(&hidden_initial_lines);
        let hidden_initial_footer = hidden_initial_lines
            .last()
            .expect("hidden selection screen should have footer");
        assert!(
            hidden_initial_footer.contains("1 hidden")
                || hidden_initial_footer.contains("1 pane hidden"),
            "{hidden_initial_screen}"
        );
        assert!(
            hidden_initial_footer.contains("backspace show all"),
            "{hidden_initial_screen}"
        );
        for inert in ["Enter output", "G show", "Space add"] {
            assert!(
                !hidden_initial_footer.contains(inert),
                "hidden selection footer advertised inert `{inert}`:\n{hidden_initial_screen}"
            );
        }
        for key in [
            KeyCode::Enter,
            KeyCode::Char(' '),
            KeyCode::Char('g'),
            KeyCode::Char('a'),
        ] {
            press(&mut hidden_top_level, key);
        }
        let hidden_after_lines = render_grid(&hidden_top_level, 104, 20);
        let hidden_after_screen = screen_text(&hidden_after_lines);
        assert_render_invariants(&hidden_after_lines, 104, 20);
        assert!(
            !hidden_top_level.is_output_view_active(),
            "{hidden_after_screen}"
        );
        assert!(
            hidden_after_screen.contains("1 hidden")
                || hidden_after_screen.contains("1 pane hidden"),
            "{hidden_after_screen}"
        );
        assert!(
            hidden_after_screen.contains("backspace show all"),
            "{hidden_after_screen}"
        );
        assert!(
            hidden_after_screen.contains("Show all panes before"),
            "{hidden_after_screen}"
        );
        assert!(
            !log_path.exists()
                || std::fs::read_to_string(&log_path)
                    .expect("fake tmux log should be readable")
                    .is_empty(),
            "hidden pane-only actions should not call tmux"
        );

        let mut hidden_more = hidden_app();
        press(&mut hidden_more, KeyCode::Char('.'));
        let hidden_more_before = render_grid(&hidden_more, 104, 20);
        for key in [
            KeyCode::Enter,
            KeyCode::Char('z'),
            KeyCode::Char('e'),
            KeyCode::Char('y'),
            KeyCode::Char('n'),
            KeyCode::Char('+'),
            KeyCode::Char('b'),
            KeyCode::Char('c'),
        ] {
            press(&mut hidden_more, key);
            assert_eq!(
                render_grid(&hidden_more, 104, 20),
                hidden_more_before,
                "unlisted hidden-pane More key {key:?} must not mutate or close the menu"
            );
        }

        app.set_target_groups_for_test(vec![
            crate::app::TargetGroup {
                name: String::from("alpha fleet"),
                members: vec![crate::app::PaneLocator {
                    session_name: String::from("demo"),
                    window_name: String::from("alpha"),
                    pane_index: 0,
                }],
            },
            crate::app::TargetGroup {
                name: String::from("beta fleet"),
                members: vec![crate::app::PaneLocator {
                    session_name: String::from("demo"),
                    window_name: String::from("beta"),
                    pane_index: 1,
                }],
            },
        ]);
        app.open_fleet_picker();
        let picker_before = render_grid(&app, 104, 20);
        press(&mut app, KeyCode::Left);
        assert!(app.is_fleet_picker_active());
        assert_eq!(
            render_grid(&app, 104, 20),
            picker_before,
            "unlisted keys must not mutate the Fleets picker"
        );
    }

    #[test]
    fn usability_state_transitions_preserve_task_recovery_and_copy_contracts() {
        let mut first = sample_pane("codex");
        first.id = String::from("%1");
        let mut second = sample_pane("claude");
        second.id = String::from("%2");
        second.active = false;
        second.pane_index = 1;
        let mut app = app_with_panes(
            vec![first, second],
            vec![
                (
                    "%1",
                    vec!["STATUS=waiting | BLOCKER=approval | NEXT=approve deploy"],
                ),
                (
                    "%2",
                    vec!["STATUS=running | BLOCKER=none | NEXT=write tests"],
                ),
            ],
        );
        let runtime = tokio::runtime::Runtime::new().expect("runtime should build");

        let assert_contract =
            |app: &App, name: &str, required: &[&str], footer_required: &[&str]| {
                let lines = render_grid(app, 110, 20);
                let screen = screen_text(&lines);
                let footer = lines.last().expect("screen should have footer");
                assert_render_invariants(&lines, 110, 20);
                assert_screen_has_one_line_chrome(name, &lines);
                assert_no_low_value_copy(name, &lines);
                for term in required {
                    assert!(screen.contains(term), "{name} missing `{term}`:\n{screen}");
                }
                for term in footer_required {
                    assert!(
                        footer.contains(term),
                        "{name} footer missing `{term}`:\n{screen}"
                    );
                }
                for forbidden in [
                    "NEXT=",
                    "STATUS=",
                    "inspect",
                    "staged",
                    "target panes",
                    "unknown",
                ] {
                    assert!(
                        !screen.contains(forbidden),
                        "{name} leaked `{forbidden}`:\n{screen}"
                    );
                }
            };

        assert_contract(
            &app,
            "initial triage",
            &["Fleet", "Details", "Blocked: approval", "Action: : reply"],
            &["? help", "Enter output", ": reply", "/ filter"],
        );

        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Char('/')))
            .expect("search should open");
        for ch in "zznomatch".chars() {
            runtime
                .block_on(handle_key_press(&mut app, KeyCode::Char(ch)))
                .expect("search typing should work");
        }
        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Enter))
            .expect("search should apply");
        assert_contract(
            &app,
            "empty search",
            &[
                "no matches",
                "No matching panes.",
                "Action: backspace show all panes",
            ],
            &["? help", "backspace show all"],
        );

        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Backspace))
            .expect("backspace should recover all panes");
        assert_contract(
            &app,
            "empty search recovery",
            &["Fleet", "Details", "Action: : reply"],
            &["? help", "Enter output", ": reply", "/ filter"],
        );

        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Char(' ')))
            .expect("mark first pane should work");
        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Char('j')))
            .expect("move should work");
        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Char(' ')))
            .expect("mark second pane should work");
        assert_contract(
            &app,
            "send list ready",
            &["send list 2 panes", "Space remove", "X clear"],
            &["? help", "send list 2 panes", ": send"],
        );

        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Char(':')))
            .expect("send should open");
        for ch in "echo hello".chars() {
            runtime
                .block_on(handle_key_press(&mut app, KeyCode::Char(ch)))
                .expect("command typing should work");
        }
        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Enter))
            .expect("send should move to review");
        assert_contract(
            &app,
            "send review",
            &["Review send to the send list (2 panes)", "Text: echo hello"],
            &["? help", "Enter send", "Esc cancel"],
        );
        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Esc))
            .expect("review cancel should preserve send list");
        assert_contract(
            &app,
            "send review cancel",
            &["send list 2 panes", "Space remove", "X clear"],
            &["? help", "send list 2 panes", ": send"],
        );

        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Char('.')))
            .expect("more should open");
        assert_contract(
            &app,
            "more open",
            &["More", "Action: : send list", "Send List", "Pane"],
            &["? help", "press a listed key", "Esc close"],
        );
        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Esc))
            .expect("more should close");
        assert_contract(
            &app,
            "more close",
            &["send list 2 panes", "Space remove", "X clear"],
            &["? help", "send list 2 panes", ": send"],
        );
    }

    #[test]
    fn usability_send_list_shows_hidden_targets_before_send() {
        let mut first = sample_pane("codex");
        first.id = String::from("%1");
        first.window_id = String::from("@1");
        first.window_name = String::from("alpha");

        let mut second = sample_pane("claude");
        second.id = String::from("%2");
        second.window_id = String::from("@2");
        second.window_name = String::from("beta");
        second.active = false;
        second.pane_index = 1;

        let mut app = app_with_panes(vec![first, second], vec![]);
        app.toggle_selected_mark();
        app.select_next_pane();
        app.toggle_selected_mark();
        app.set_search_query_for_test("alpha");
        app.begin_command_input();
        for ch in "echo {id}".chars() {
            app.push_command_char(ch);
        }

        let lines = render_grid(&app, 110, 22);
        let screen = screen_text(&lines);

        assert_render_invariants(&lines, 110, 22);
        assert!(screen.contains("Send"), "{screen}");
        assert!(screen.contains("send list (2 panes, 1 hidden)"), "{screen}");
        assert!(screen.contains("1 pane hidden by current view"), "{screen}");
        assert!(
            screen.contains("demo / beta (hidden) : echo %2"),
            "{screen}"
        );
        assert!(screen.contains("Enter review"), "{screen}");

        let short_lines = render_grid(&app, 88, 14);
        let short_screen = screen_text(&short_lines);
        assert_render_invariants(&short_lines, 88, 14);
        assert!(
            short_screen.contains("send list (2 panes, 1 hidden)"),
            "{short_screen}"
        );
        assert!(
            short_screen.contains("1 pane hidden by current view"),
            "{short_screen}"
        );
        assert!(
            !short_lines
                .iter()
                .any(|line| line.trim() == "send list 2 panes"),
            "{short_screen}"
        );
    }

    #[test]
    fn usability_send_panel_recent_repeat_affordance_is_truthful() {
        let pane = sample_pane("codex");
        let mut app = app_with_panes(vec![pane], vec![]);
        remember_command_for_test(&mut app, "older command");
        remember_command_for_test(&mut app, "cargo test");
        app.begin_command_input();

        let lines = render_grid(&app, 100, 20);
        let screen = screen_text(&lines);

        assert_render_invariants(&lines, 100, 20);
        assert!(screen.contains("Send"), "{screen}");
        assert!(screen.contains("Recent"), "{screen}");
        assert!(screen.contains("] repeat cargo test"), "{screen}");
        assert!(!screen.contains("older command"), "{screen}");
        assert!(screen.contains("] repeat latest"), "{screen}");
        assert!(!screen.contains("Macros"), "{screen}");

        app.push_command_char('x');
        let typed_lines = render_grid(&app, 100, 20);
        let typed_screen = screen_text(&typed_lines);

        assert_render_invariants(&typed_lines, 100, 20);
        assert!(
            !typed_screen.contains("] repeat cargo test"),
            "{typed_screen}"
        );
        assert!(!typed_screen.contains("] repeat latest"), "{typed_screen}");
    }

    #[test]
    fn usability_reply_composer_is_not_mislabeled_as_generic_send() {
        let pane = sample_pane("claude");
        let mut app = app_with_panes(vec![pane], vec![("%1", vec!["approval needed"])]);
        remember_command_for_test(&mut app, "cargo test");
        app.begin_command_input();

        let lines = render_grid(&app, 100, 20);
        let screen = screen_text(&lines);

        assert_render_invariants(&lines, 100, 20);
        assert!(screen.contains("Reply to demo / agents."), "{screen}");
        assert!(screen.contains("┌Reply"), "{screen}");
        assert!(screen.contains("Reply to: demo / agents"), "{screen}");
        assert!(screen.contains("Enter reply"), "{screen}");
        assert!(!screen.contains("Send to demo / agents."), "{screen}");
        assert!(!screen.contains("Enter send"), "{screen}");
        assert!(!screen.contains("Recent"), "{screen}");
        assert!(!screen.contains("] repeat"), "{screen}");

        let title_y = line_index(&lines, "┌Reply");
        let target_y = line_index(&lines, "Reply to: demo / agents");
        let text_y = line_index(&lines, "Text: _");
        assert_eq!(target_y, title_y + 1, "{screen}");
        assert_eq!(text_y, target_y + 1, "{screen}");
    }

    #[test]
    fn usability_command_shortcuts_only_appear_on_the_send_surface() {
        let pane = sample_pane("codex");
        let mut app = app_with_panes(vec![pane], vec![]);
        remember_command_for_test(&mut app, "cargo test");
        app.begin_macro_assign();
        app.assign_recent_command_to_slot(0);
        app.cycle_context_pane();
        app.cycle_context_pane();
        app.cycle_context_pane();

        let default_lines = render_grid(&app, 100, 20);
        let default_screen = screen_text(&default_lines);

        assert_render_invariants(&default_lines, 100, 20);
        assert!(!default_screen.contains("] repeat cargo test"));
        assert!(!default_screen.contains("Macros"));
        assert!(!default_screen.contains("1: cargo test"));

        app.begin_command_input();
        let command_lines = render_grid(&app, 100, 20);
        let command_screen = screen_text(&command_lines);

        assert_render_invariants(&command_lines, 100, 20);
        assert!(command_screen.contains("Send"), "{command_screen}");
        assert!(command_screen.contains("Recent"), "{command_screen}");
        assert!(
            command_screen.contains("] repeat cargo test"),
            "{command_screen}"
        );
        assert!(!command_screen.contains("Macros"), "{command_screen}");
        assert!(
            !command_screen.contains("1: cargo test"),
            "{command_screen}"
        );
        app.cancel_command_input();

        app.cycle_context_pane();
        app.cycle_context_pane();
        let send_lines = render_grid(&app, 100, 20);
        let send_screen = screen_text(&send_lines);

        assert_render_invariants(&send_lines, 100, 20);
        assert!(send_screen.contains("Send"), "{send_screen}");
        assert!(send_screen.contains("Recent"), "{send_screen}");
        assert!(send_screen.contains("] repeat cargo test"), "{send_screen}");
        assert!(send_screen.contains("Macros"), "{send_screen}");
        assert!(send_screen.contains("1: cargo test"), "{send_screen}");
    }

    #[test]
    fn usability_review_send_hides_inactive_command_shortcuts() {
        let mut first = sample_pane("codex");
        first.id = String::from("%1");
        let mut second = sample_pane("codex");
        second.id = String::from("%2");
        second.active = false;
        second.pane_index = 1;
        let mut app = app_with_panes(vec![first, second], vec![]);
        app.toggle_selected_mark();
        app.select_next_pane();
        app.toggle_selected_mark();
        remember_command_for_test(&mut app, "cargo test");
        app.begin_macro_assign();
        app.assign_recent_command_to_slot(0);
        app.begin_command_input();
        for ch in "echo hi".chars() {
            app.push_command_char(ch);
        }
        let runtime = tokio::runtime::Runtime::new().expect("runtime should build");
        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Enter))
            .expect("multi-target command should stage");

        let lines = render_grid(&app, 100, 20);
        let screen = screen_text(&lines);

        assert_render_invariants(&lines, 100, 20);
        assert!(screen.contains("Review send"), "{screen}");
        assert!(!screen.contains("] repeat cargo test"), "{screen}");
        assert!(!screen.contains("Macros"), "{screen}");
        assert!(!screen.contains("1: cargo test"), "{screen}");
    }

    #[test]
    fn usability_review_send_keeps_hidden_targets_visible_before_dispatch() {
        let mut first = sample_pane("codex");
        first.id = String::from("%1");
        first.window_id = String::from("@1");
        first.window_name = String::from("alpha");

        let mut second = sample_pane("claude");
        second.id = String::from("%2");
        second.window_id = String::from("@2");
        second.window_name = String::from("beta");
        second.active = false;
        second.pane_index = 1;

        let mut app = app_with_panes(vec![first, second], vec![]);
        app.toggle_selected_mark();
        app.select_next_pane();
        app.toggle_selected_mark();
        app.set_search_query_for_test("alpha");
        app.begin_command_input();
        for ch in "echo {id}".chars() {
            app.push_command_char(ch);
        }
        tokio::runtime::Runtime::new()
            .expect("runtime should build")
            .block_on(app.submit_command_input())
            .expect("submit should stage");

        let lines = render_grid(&app, 88, 14);
        let screen = screen_text(&lines);

        assert_render_invariants(&lines, 88, 14);
        assert!(screen.contains("Review send"), "{screen}");
        assert!(
            screen.contains("To: the send list (2 panes, 1 hidden)"),
            "{screen}"
        );
        assert!(screen.contains("1 pane hidden by current view"), "{screen}");
        assert!(screen.contains("demo / beta (hidden) echo %2"), "{screen}");
        assert!(screen.contains("Enter send"), "{screen}");
        assert!(screen.contains("Esc cancel"), "{screen}");
    }

    #[test]
    fn usability_review_send_prefers_hidden_example_when_targets_overflow() {
        let mut first = sample_pane("codex");
        first.id = String::from("%1");
        first.window_id = String::from("@1");
        first.window_name = String::from("alpha");

        let mut second = sample_pane("codex");
        second.id = String::from("%2");
        second.window_id = String::from("@2");
        second.window_name = String::from("beta");
        second.active = false;
        second.pane_index = 1;

        let mut third = sample_pane("codex");
        third.id = String::from("%3");
        third.window_id = String::from("@3");
        third.window_name = String::from("gamma");
        third.active = false;
        third.pane_index = 2;

        let mut hidden = sample_pane("claude");
        hidden.id = String::from("%4");
        hidden.window_id = String::from("@4");
        hidden.window_name = String::from("zeta");
        hidden.active = false;
        hidden.pane_index = 3;

        let mut app = app_with_panes(vec![first, second, third, hidden], vec![]);
        app.toggle_selected_mark();
        app.select_next_pane();
        app.toggle_selected_mark();
        app.select_next_pane();
        app.toggle_selected_mark();
        app.select_next_pane();
        app.toggle_selected_mark();
        app.set_search_query_for_test("codex");
        app.begin_command_input();
        for ch in "echo {window}".chars() {
            app.push_command_char(ch);
        }
        tokio::runtime::Runtime::new()
            .expect("runtime should build")
            .block_on(app.submit_command_input())
            .expect("submit should stage");

        let lines = render_grid(&app, 80, 14);
        let screen = screen_text(&lines);

        assert_render_invariants(&lines, 80, 14);
        assert!(screen.contains("1 pane hidden by current view"), "{screen}");
        assert!(
            screen.contains("demo / zeta (hidden) echo zeta"),
            "{screen}"
        );
        assert!(screen.contains("... 2 more"), "{screen}");
        assert!(!screen.contains("demo / beta echo beta"), "{screen}");
    }

    #[test]
    fn usability_saved_fleet_shows_hidden_targets_without_duplicate_name() {
        let mut first = sample_pane("codex");
        first.id = String::from("%1");
        first.window_id = String::from("@1");
        first.window_name = String::from("alpha");

        let mut second = sample_pane("claude");
        second.id = String::from("%2");
        second.window_id = String::from("@2");
        second.window_name = String::from("beta");
        second.active = false;
        second.pane_index = 1;

        let mut app = app_with_panes(vec![first, second], vec![]);
        app.set_target_groups_for_test(vec![crate::app::TargetGroup {
            name: String::from("triage"),
            members: vec![
                crate::app::PaneLocator {
                    session_name: String::from("demo"),
                    window_name: String::from("alpha"),
                    pane_index: 0,
                },
                crate::app::PaneLocator {
                    session_name: String::from("demo"),
                    window_name: String::from("beta"),
                    pane_index: 1,
                },
            ],
        }]);
        app.load_next_target_group();
        app.set_search_query_for_test("alpha");
        app.begin_command_input();
        for ch in "echo {window}".chars() {
            app.push_command_char(ch);
        }

        let lines = render_grid(&app, 88, 14);
        let screen = screen_text(&lines);

        assert_render_invariants(&lines, 88, 14);
        assert!(
            screen.contains("To: fleet triage (2 panes, 1 hidden)"),
            "{screen}"
        );
        assert!(screen.contains("Text: echo {window}"), "{screen}");
        assert!(screen.contains("1 pane hidden by current view"), "{screen}");
        assert!(
            screen.contains("  demo / beta (hidden) : echo beta"),
            "{screen}"
        );
        assert!(!screen.contains("│ fleet triage"), "{screen}");
    }

    #[test]
    fn usability_more_menu_is_contextual_instead_of_an_action_dump() {
        let assert_more = |app: &App, name: &str, required: &[&str], forbidden: &[&str]| {
            let lines = render_grid(app, 120, 28);
            let screen = screen_text(&lines);
            let footer = lines.last().expect("screen should have footer");
            assert_render_invariants(&lines, 120, 28);
            assert_screen_has_one_line_chrome(name, &lines);
            assert_no_low_value_copy(name, &lines);
            assert!(screen.contains("More"), "{name}\n{screen}");
            assert!(screen.contains("Action: "), "{name}\n{screen}");
            assert!(screen.contains("View"), "{name}\n{screen}");
            assert!(footer.contains("press a listed key"), "{name}\n{screen}");
            assert!(footer.contains("Esc close"), "{name}\n{screen}");
            for term in required {
                assert!(screen.contains(term), "{name} missing `{term}`:\n{screen}");
            }
            for term in forbidden {
                assert!(
                    !screen.contains(term),
                    "{name} should not show `{term}`:\n{screen}"
                );
            }
        };

        let mut idle = app_with_panes(vec![sample_pane("bash")], vec![]);
        idle.open_action_menu();
        assert_more(
            &idle,
            "idle More",
            &[
                "Action: : send this pane",
                "Enter show output",
                "[ browse windows",
                "] command center",
                "Pane",
                "G show in tmux",
                "Z zoom pane",
                "E send Enter",
            ],
            &[
                "continue waiting",
                "answer yes",
                "answer no",
                "mute alert",
                "clear send list",
            ],
        );

        let mut waiting = app_with_panes(
            vec![sample_pane("codex")],
            vec![("%1", vec!["Press Enter to continue."])],
        );
        waiting.open_action_menu();
        assert_more(
            &waiting,
            "waiting More",
            &["Pane", "I continue waiting", "Z zoom pane"],
            &["answer yes", "answer no", "unmute alert"],
        );
        let waiting_lines = render_grid(&waiting, 120, 28);
        let waiting_screen = screen_text(&waiting_lines);
        assert!(
            line_index(&waiting_lines, "I continue waiting")
                < line_index(&waiting_lines, "Z zoom pane"),
            "{waiting_screen}"
        );

        let mut reply = app_with_panes(
            vec![sample_pane("codex")],
            vec![("%1", vec!["Type your answer to continue."])],
        );
        reply.open_action_menu();
        let reply_lines = render_grid(&reply, 120, 28);
        let reply_screen = screen_text(&reply_lines);
        assert_more(
            &reply,
            "reply More",
            &["Action: : reply", "View", ": reply", "G show in tmux"],
            &[
                "Action: C mute alert",
                ": send text",
                "answer yes",
                "answer no",
            ],
        );
        assert!(
            line_index(&reply_lines, "Action: : reply") < line_index(&reply_lines, "│   : reply"),
            "{reply_screen}"
        );

        let mut waiting_pane = sample_pane("codex");
        waiting_pane.id = String::from("%1");
        waiting_pane.active = false;
        let mut running_pane = sample_pane("node");
        running_pane.id = String::from("%2");
        running_pane.pane_index = 1;
        let mut mixed = app_with_panes(
            vec![waiting_pane, running_pane],
            vec![("%1", vec!["Press Enter to continue."])],
        );
        mixed.select_next_pane();
        mixed.open_action_menu();
        let mixed_lines = render_grid(&mixed, 120, 28);
        let mixed_screen = screen_text(&mixed_lines);
        assert!(
            mixed_screen.contains("I continue waiting panes"),
            "{mixed_screen}"
        );
        assert!(
            !mixed_lines
                .iter()
                .any(|line| line.trim() == "I continue waiting"),
            "bulk continue should not look selected-pane specific:\n{mixed_screen}"
        );

        let mut choice = app_with_panes(
            vec![sample_pane("claude")],
            vec![("%1", vec!["Approve network access? [y/n]"])],
        );
        choice.open_action_menu();
        let choice_lines = render_grid(&choice, 120, 28);
        let choice_screen = screen_text(&choice_lines);
        assert_more(
            &choice,
            "choice More",
            &["Y answer yes", "N answer no", "C mute alert"],
            &["unmute alert"],
        );
        assert_eq!(
            choice_screen.matches("│   Y answer yes").count(),
            1,
            "{choice_screen}"
        );
        assert!(
            choice_screen.contains("Action: Y answer yes, N answer no"),
            "{choice_screen}"
        );
        assert!(
            line_index(&choice_lines, "Action: ") < line_index(&choice_lines, "│   Y answer yes"),
            "{choice_screen}"
        );
        assert!(
            line_index(&choice_lines, "│   N answer no")
                < line_index(&choice_lines, "│   C mute alert"),
            "{choice_screen}"
        );

        let mut report = app_with_panes(
            vec![sample_pane("claude")],
            vec![("%1", vec!["Continue? [y/n]"])],
        );
        set_pane_report_fields(
            &mut report,
            "%1",
            "waiting",
            "approval needed",
            "answer yes or no",
        );
        report.open_action_menu();
        let report_lines = render_grid(&report, 120, 28);
        let report_screen = screen_text(&report_lines);
        assert_render_invariants(&report_lines, 120, 28);
        assert!(report_screen.contains("Reports"), "{report_screen}");
        assert!(
            report_screen.contains("demo / agents: waiting | approval needed | answer yes or no"),
            "{report_screen}"
        );
        assert!(!report_screen.contains("%1"), "{report_screen}");

        let mut ssh_report = app_with_panes(
            vec![sample_pane("codex")],
            vec![("%1", vec!["Waiting for approval. Continue?"])],
        );
        use_notification_mode_for_test(
            &mut ssh_report,
            crate::notifications::NotificationMode::SshFallback,
        );
        set_pane_report_fields(
            &mut ssh_report,
            "%1",
            "waiting",
            "approval needed",
            "approve",
        );
        ssh_report.open_action_menu();
        let ssh_lines = render_grid(&ssh_report, 80, 24);
        let ssh_screen = screen_text(&ssh_lines);
        assert_render_invariants(&ssh_lines, 80, 24);
        assert!(ssh_screen.contains("Reports"), "{ssh_screen}");
        assert!(
            ssh_screen.contains("demo / agents: waiting | approval needed | approve"),
            "{ssh_screen}"
        );
        assert!(
            ssh_screen.contains("desktop alerts unavailable on SSH"),
            "{ssh_screen}"
        );
        assert!(ssh_screen.contains("V terminal bell"), "{ssh_screen}");

        let mut marked = app_with_panes(vec![sample_pane("codex")], vec![]);
        marked.toggle_selected_mark();
        marked.open_action_menu();
        assert_more(
            &marked,
            "marked More",
            &[
                "Action: : send list",
                "Send List",
                "X clear send list",
                "G save fleet",
            ],
            &["answer yes", "answer no"],
        );

        let mut hidden_marked = app_with_panes(vec![sample_pane("codex")], vec![]);
        hidden_marked.toggle_selected_mark();
        hidden_marked.set_search_query_for_test("zz-no-match");
        hidden_marked.open_action_menu();
        assert_more(
            &hidden_marked,
            "hidden marked More",
            &[
                "Action: : send list",
                "send list (1 pane, 1 hidden)",
                "backspace show all panes",
                ": send text",
                "X clear send list",
            ],
            &["Enter show output", "Z zoom pane", "Space add"],
        );

        let stale_fixture: ViewModelFixture = serde_json::from_value(serde_json::json!({
            "name": "stale marked",
            "marked_pane_ids": ["%missing"]
        }))
        .expect("stale marked fixture should parse");
        let mut stale_marked = app_from_view_model_fixture(&stale_fixture);
        stale_marked.open_action_menu();
        assert_more(
            &stale_marked,
            "stale marked More",
            &[
                "Action: Space add a visible pane",
                "send list has no live panes",
                "X clear send list",
            ],
            &[": send text", "S summarize panes", "G save fleet"],
        );
    }

    #[test]
    fn usability_primary_surfaces_hide_raw_tmux_identity() {
        fn app_with_raw_tmux_identity() -> App {
            let mut first = sample_pane("codex");
            first.id = String::from("%1");
            first.session_id = String::from("$0");
            first.window_id = String::from("@0");
            let mut second = sample_pane("claude");
            second.id = String::from("%2");
            second.session_id = String::from("$1");
            second.window_id = String::from("@1");
            second.window_name = String::from("review");
            second.active = false;
            second.pane_index = 1;

            let mut app = app_with_panes(
                vec![first, second],
                vec![
                    ("%1", vec!["Approve deploy? [y/n]"]),
                    ("%2", vec!["Running integration tests"]),
                ],
            );
            set_pane_report_fields(
                &mut app,
                "%1",
                "waiting",
                "approval needed",
                "answer yes or no",
            );
            app
        }

        let mut surfaces: Vec<(&str, App)> = Vec::new();

        surfaces.push(("home", app_with_raw_tmux_identity()));

        let mut output = app_with_raw_tmux_identity();
        output.cycle_context_pane();
        surfaces.push(("output", output));

        let mut send = app_with_raw_tmux_identity();
        send.show_send_view();
        surfaces.push(("send", send));

        let mut browse = app_with_raw_tmux_identity();
        browse.show_browse_view();
        surfaces.push(("browse", browse));

        let mut command_center = app_with_raw_tmux_identity();
        command_center.show_command_center();
        surfaces.push(("command center", command_center));

        let mut more = app_with_raw_tmux_identity();
        more.open_action_menu();
        surfaces.push(("more", more));

        let mut help = app_with_raw_tmux_identity();
        help.toggle_help_overlay();
        surfaces.push(("help", help));

        let mut marked = app_with_raw_tmux_identity();
        marked.toggle_selected_mark();
        surfaces.push(("marked", marked));

        for (label, app) in surfaces {
            let lines = render_grid(&app, 120, 28);
            assert_render_invariants(&lines, 120, 28);
            assert_screen_hides_raw_tmux_identity(label, &lines);
        }
    }

    #[test]
    fn usability_rendered_surfaces_honor_rebound_keys_without_stale_defaults() {
        let mut app = app_with_panes(
            vec![sample_pane("codex")],
            vec![("%1", vec!["Press Enter to continue."])],
        );
        apply_rebound_keybindings(&mut app);

        let board_lines = render_grid(&app, 140, 20);
        let board_screen = screen_text(&board_lines);
        let board_footer = board_lines.last().expect("board should have footer");
        assert_render_invariants(&board_lines, 140, 20);
        for term in [
            "N/P move", "O output", "L show", "V add", "C send", "F filter", "M more", "Z quit",
        ] {
            assert!(
                board_footer.contains(term),
                "footer missing rebound `{term}`:\n{board_screen}"
            );
        }
        for stale in [
            "J/K move",
            "Enter output",
            "G show",
            "Space add",
            ": send",
            "/ filter",
            ". more",
            "Q quit",
            "L layout",
        ] {
            assert!(
                !board_footer.contains(stale),
                "footer leaked stale default `{stale}`:\n{board_screen}"
            );
        }

        app.toggle_help_overlay();
        let help_lines = render_grid(&app, 120, 20);
        let help_screen = screen_text(&help_lines);
        assert_render_invariants(&help_lines, 120, 20);
        for term in [
            "Now: O output, L show in tmux, U continue waiting.",
            "Send: C send text, V add/remove pane.",
            "Find: F filter",
            "Move: N/P select panes",
            "Views: M then [ browse, ] command center; 8 layout.",
            "More: M then + start agent, 0 zoom pane.",
            "Close: Esc backs out or closes Help, Z quit muxboard.",
        ] {
            assert!(
                help_screen.contains(term),
                "help missing `{term}`:\n{help_screen}"
            );
        }
        for stale in [
            "Enter output",
            "G show",
            "Space add",
            ": command",
            "/ filter",
            ". sort",
            "Q quit",
        ] {
            assert!(
                !help_screen.contains(stale),
                "help leaked stale default `{stale}`:\n{help_screen}"
            );
        }
        app.close_help_overlay();

        app.open_action_menu();
        let more_lines = render_grid(&app, 120, 28);
        let more_screen = screen_text(&more_lines);
        assert_render_invariants(&more_lines, 120, 28);
        for term in [
            "Action: ; continue waiting panes",
            "; continue waiting",
            "0 zoom pane",
        ] {
            assert!(
                more_screen.contains(term),
                "More missing `{term}`:\n{more_screen}"
            );
        }
        for stale in ["Action: Space", "I continue waiting", "Z zoom pane"] {
            assert!(
                !more_screen.contains(stale),
                "More leaked stale default `{stale}`:\n{more_screen}"
            );
        }
    }

    #[test]
    fn usability_responsive_matrix_preserves_the_minimum_useful_surface() {
        let output = app_from_panel_fixture(&panel_fixture("live_tail_with_summary_and_raw_tail"));
        let scenarios = vec![
            (
                "selected",
                app_from_panel_fixture(&panel_fixture("selected_waiting_panel")),
                vec!["Fleet", "Details"],
                vec!["Action:"],
                vec!["? help", "Enter output"],
                vec!["No output yet.", "unknown", "NEXT="],
            ),
            (
                "output",
                output,
                vec!["Output"],
                vec!["Summary", "write tests"],
                vec!["? help", "Esc back"],
                vec!["No output yet.", "unknown", "NEXT="],
            ),
            (
                "send",
                app_from_view_model_fixture(&view_fixture("command_input_context")),
                vec!["Send to", "Send"],
                vec!["To:", "Text:", "Preview"],
                vec!["? help", "Enter send", "Esc cancel"],
                vec!["staged", "target", "vars {session}"],
            ),
            (
                "review",
                app_from_view_model_fixture(&view_fixture("confirm_dispatch_header_and_footer")),
                vec!["Review send", "Send"],
                vec!["Enter send", "Esc cancel"],
                vec!["? help", "Enter send", "Esc cancel"],
                vec!["staged", "target panes", "Ready to send"],
            ),
            (
                "empty browse",
                app_from_panel_fixture(&panel_fixture("navigator_empty_state")),
                vec!["Browse"],
                vec!["No matching panes.", "Action: backspace show all panes"],
                vec!["? help"],
                vec!["Nothing here.", "unknown", "No panes in view."],
            ),
        ];

        for (name, app, always_terms, roomy_terms, footer_terms, forbidden_terms) in scenarios {
            for &(width, height) in &[(60, 14), (80, 18), (100, 24), (140, 36)] {
                let lines = render_grid(&app, width, height);
                let screen = screen_text(&lines);
                let footer = lines.last().expect("screen should have footer");
                assert_render_invariants(&lines, width, height);
                assert_screen_has_one_line_chrome(name, &lines);
                assert_no_low_value_copy(name, &lines);
                for term in &always_terms {
                    assert!(
                        screen.contains(term),
                        "{name} {width}x{height} should preserve `{term}`:\n{screen}"
                    );
                }
                if width >= 80 {
                    for term in &roomy_terms {
                        assert!(
                            screen.contains(term),
                            "{name} {width}x{height} should preserve `{term}`:\n{screen}"
                        );
                    }
                }
                for term in &forbidden_terms {
                    assert!(
                        !screen.contains(term),
                        "{name} {width}x{height} should hide `{term}`:\n{screen}"
                    );
                }
                for term in &footer_terms {
                    let text_entry_footer =
                        footer.contains("type ") && footer.contains("Esc cancel");
                    if text_entry_footer && *term == "? help" {
                        assert!(
                            !footer.contains(term),
                            "{name} {width}x{height} footer should not advertise ? help while ? is text:\n{screen}"
                        );
                    } else if width >= 80 || *term == "? help" {
                        assert!(
                            footer.contains(term),
                            "{name} {width}x{height} footer should preserve `{term}`:\n{screen}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn usability_information_budget_spends_space_on_decisions_not_chatter() {
        let selected = app_from_panel_fixture(&panel_fixture("selected_waiting_panel"));
        let selected_lines = render_grid(&selected, 100, 14);
        let selected_screen = screen_text(&selected_lines);
        assert_render_invariants(&selected_lines, 100, 14);
        assert_screen_has_one_line_chrome("selected budget", &selected_lines);
        assert_no_low_value_copy("selected budget", &selected_lines);
        assert!(selected_screen.contains("Action:"), "{selected_screen}");
        assert!(
            line_index(&selected_lines, "Action:") < line_index(&selected_lines, "Output"),
            "{selected_screen}"
        );

        let empty = app_from_panel_fixture(&panel_fixture("navigator_empty_state"));
        let empty_lines = render_grid(&empty, 80, 16);
        let empty_screen = screen_text(&empty_lines);
        assert_render_invariants(&empty_lines, 80, 16);
        assert_screen_has_one_line_chrome("empty budget", &empty_lines);
        assert_no_low_value_copy("empty budget", &empty_lines);
        assert_eq!(
            empty_screen.matches("No matching panes.").count(),
            1,
            "{empty_screen}"
        );
        assert_eq!(
            empty_screen
                .matches("Action: backspace show all panes")
                .count(),
            1,
            "{empty_screen}"
        );
        assert!(
            line_index(&empty_lines, "No matching panes.")
                < line_index(&empty_lines, "Action: backspace show all panes"),
            "{empty_screen}"
        );

        let mut help = app_from_view_model_fixture(&view_fixture("attention_board_row_summary"));
        help.toggle_help_overlay();
        let help_lines = render_grid(&help, 100, 18);
        let help_screen = screen_text(&help_lines);
        assert_render_invariants(&help_lines, 100, 18);
        assert_screen_has_one_line_chrome("help budget", &help_lines);
        assert!(
            help_screen.matches("│").count() <= 18,
            "help should stay short enough to scan:\n{help_screen}"
        );
        assert!(!help_screen.contains("tutorial"), "{help_screen}");
    }

    #[test]
    fn usability_provider_rows_answer_what_is_happening_without_tmux_guesswork() {
        let mut codex = sample_pane("codex");
        codex.id = String::from("%1");
        codex.window_name = String::from("codex");
        let mut claude = sample_pane("claude");
        claude.id = String::from("%2");
        claude.window_name = String::from("claude");
        claude.active = false;
        claude.pane_index = 1;
        let mut opencode = sample_pane("opencode");
        opencode.id = String::from("%3");
        opencode.window_name = String::from("opencode");
        opencode.active = false;
        opencode.pane_index = 2;
        let mut shell = sample_pane("bash");
        shell.id = String::from("%4");
        shell.window_name = String::from("shell");
        shell.active = false;
        shell.pane_index = 3;

        let app = app_with_panes(
            vec![codex, claude, opencode, shell],
            vec![
                ("%1", vec!["STATUS=running | BLOCKER=none | NEXT=ship fix"]),
                ("%2", vec!["Waiting for approval to use network"]),
                ("%3", vec!["Type your answer..."]),
                ("%4", vec!["cargo test --workspace", "test result: ok"]),
            ],
        );
        let lines = render_grid(&app, 200, 24);
        let screen = screen_text(&lines);

        assert_render_invariants(&lines, 200, 24);
        assert_screen_has_one_line_chrome("provider semantics", &lines);
        assert_no_low_value_copy("provider semantics", &lines);
        for term in [
            "codex",
            "ship fix",
            "claude",
            "approval",
            "input needed",
            "shell",
            "test result: ok",
        ] {
            assert!(screen.contains(term), "missing `{term}`:\n{screen}");
        }
        assert!(
            line_index(&lines, "codex") < line_index(&lines, "ship fix"),
            "{screen}"
        );
    }

    #[test]
    fn usability_primary_action_is_prominent_and_recoverable() {
        let selected = app_from_panel_fixture(&panel_fixture("selected_waiting_panel"));
        let selected_lines = render_grid(&selected, 100, 18);
        let selected_screen = screen_text(&selected_lines);
        assert_render_invariants(&selected_lines, 100, 18);
        assert!(
            selected_screen.contains("Action: : reply"),
            "{selected_screen}"
        );
        assert!(
            line_index(&selected_lines, "Action:") < line_index(&selected_lines, "Output"),
            "{selected_screen}"
        );

        let narrow_lines = render_grid(&selected, 70, 16);
        let narrow_screen = screen_text(&narrow_lines);
        assert_render_invariants(&narrow_lines, 70, 16);
        assert!(narrow_screen.contains("Action: : reply"), "{narrow_screen}");
        assert!(narrow_screen.contains("? help"), "{narrow_screen}");

        let more = app_from_panel_fixture(&panel_fixture("actions_menu_sections"));
        let more_lines = render_grid(&more, 100, 18);
        let more_screen = screen_text(&more_lines);
        assert_render_invariants(&more_lines, 100, 18);
        assert!(more_screen.contains("Action: "), "{more_screen}");
        assert!(
            line_index(&more_lines, "Action: ") < line_index(&more_lines, "View"),
            "{more_screen}"
        );
        assert!(
            more_lines
                .last()
                .is_some_and(|line| line.contains("Esc close"))
        );

        let review =
            app_from_view_model_fixture(&view_fixture("confirm_dispatch_header_and_footer"));
        let review_lines = render_grid(&review, 100, 18);
        let review_screen = screen_text(&review_lines);
        assert_render_invariants(&review_lines, 100, 18);
        assert!(review_screen.contains("Enter send"), "{review_screen}");
        assert!(review_screen.contains("Esc cancel"), "{review_screen}");
        assert!(
            review_lines
                .last()
                .is_some_and(|line| line.contains("Enter send"))
        );
        assert!(
            review_lines
                .last()
                .is_some_and(|line| line.contains("Esc cancel"))
        );
    }

    #[test]
    fn usability_empty_states_explain_recovery_not_just_absence() {
        let browse = app_from_panel_fixture(&panel_fixture("navigator_empty_state"));

        for &(width, height) in &[(80, 16), (100, 18)] {
            let lines = render_grid(&browse, width, height);
            let screen = screen_text(&lines);
            assert_render_invariants(&lines, width, height);
            assert!(screen.contains("No matching panes."), "{screen}");
            assert!(
                screen.contains("Action: backspace show all panes"),
                "{screen}"
            );
            assert!(!screen.contains("Nothing here."), "{screen}");
            assert!(screen.contains("? help"), "{screen}");
        }
    }

    #[test]
    fn tiny_empty_browse_keeps_recovery_visible_without_inert_actions() {
        let browse = app_from_panel_fixture(&panel_fixture("navigator_empty_state"));
        let lines = render_grid(&browse, 60, 12);
        let screen = screen_text(&lines);
        let footer = lines.last().expect("footer should render");

        assert_render_invariants(&lines, 60, 12);
        assert!(screen.contains("Browse"), "{screen}");
        assert!(screen.contains("No matching panes."), "{screen}");
        assert!(
            screen.contains("Action: backspace show all panes"),
            "{screen}"
        );
        assert!(footer.contains("? help"), "{screen}");
        for inert in ["Enter window", "G show", "J/K browse"] {
            assert!(
                !screen.contains(inert),
                "empty tiny Browse advertised inert `{inert}`:\n{screen}"
            );
        }
    }

    #[test]
    fn usability_provider_protocol_is_distilled_before_it_reaches_fleet_or_details() {
        let app = app_with_panes(
            vec![sample_pane("codex")],
            vec![(
                "%1",
                vec![
                    "Reply in exactly one line as: STATUS=<status> | BLOCKER=<blocker> | NEXT=<next>.",
                    "NEXT=<next>.",
                    "STATUS=running | BLOCKER=none | NEXT=ship fix",
                ],
            )],
        );

        for &(width, height) in &[(80, 16), (120, 18)] {
            let lines = render_grid(&app, width, height);
            let screen = screen_text(&lines);
            assert_render_invariants(&lines, width, height);
            assert!(
                screen.contains("Now: ship fix"),
                "{width}x{height}\n{screen}"
            );
            assert!(screen.contains("ship fix"), "{width}x{height}\n{screen}");
            assert_screen_hides_internal_protocol(&screen);
            assert_screen_avoids_retired_user_terms(&screen);
        }
    }

    #[test]
    fn usability_footer_keeps_keymap_during_navigation_and_focus_changes() {
        let mut first = sample_pane("bash");
        first.id = String::from("%1");
        let mut second = sample_pane("codex");
        second.id = String::from("%2");
        second.active = false;
        second.pane_index = 1;

        let mut app = app_with_panes(
            vec![first, second],
            vec![
                (
                    "%1",
                    vec![
                        "download dependencies",
                        "compile crate",
                        "run unit tests",
                        "package binary",
                    ],
                ),
                (
                    "%2",
                    vec!["codex", "STATUS=running | BLOCKER=none | NEXT=write tests"],
                ),
            ],
        );

        let assert_useful_footer = |app: &App, label: &str| {
            let lines = render_grid(app, 120, 18);
            let screen = screen_text(&lines);
            assert_render_invariants(&lines, 120, 18);
            let footer = lines
                .last()
                .unwrap_or_else(|| panic!("{label} rendered no footer"));
            assert!(footer.contains("? help"), "{label}\n{screen}");
            assert!(
                footer.contains("Enter output") || footer.contains("Esc back"),
                "{label}\n{screen}"
            );
            assert!(footer.contains(": send"), "{label}\n{screen}");
            assert!(footer.contains("/ filter"), "{label}\n{screen}");
            assert!(footer.contains(". more"), "{label}\n{screen}");
            assert!(
                !footer.contains("focused"),
                "{label} should not replace keymap with focus confirmation:\n{screen}"
            );
            assert!(
                !footer.contains("scrolled"),
                "{label} should not replace keymap with scroll confirmation:\n{screen}"
            );
        };

        assert_useful_footer(&app, "initial");

        app.select_next_pane();
        assert_useful_footer(&app, "after fleet move");

        app.cycle_panel_focus();
        assert_useful_footer(&app, "after details focus");

        app.select_next_pane();
        assert_useful_footer(&app, "after details scroll");

        app.open_action_menu();
        app.dismiss_action_menu();
        assert_useful_footer(&app, "after opening and closing More");
    }

    #[test]
    fn usability_narrow_send_footer_keeps_recent_repeat_action_visible() {
        let pane = sample_pane("codex");
        let mut app = app_with_panes(vec![pane], vec![]);
        remember_command_for_test(&mut app, "cargo test");
        app.begin_command_input();

        let lines = render_grid(&app, 68, 14);
        let screen = screen_text(&lines);
        let footer = lines.last().expect("footer should render");

        assert_render_invariants(&lines, 68, 14);
        assert!(screen.contains("Send"), "{screen}");
        for term in ["] repeat", "Enter send", "Esc cancel"] {
            assert!(
                footer.contains(term),
                "narrow Send footer should keep `{term}` visible:\n{screen}"
            );
        }
        assert!(
            !footer.contains("? help"),
            "? should remain typeable text while Send input is active:\n{screen}"
        );
        assert!(
            !footer.contains("backspace delete"),
            "narrow Send footer should omit lower-priority delete copy:\n{screen}"
        );
        assert_no_low_value_copy("narrow send footer", &lines);
    }

    #[test]
    fn usability_browse_feedback_footer_keeps_only_browse_actions_visible() {
        let mut first = sample_pane("codex");
        first.id = String::from("%1");
        let mut second = sample_pane("claude");
        second.id = String::from("%2");
        second.window_id = String::from("@2");
        second.window_name = String::from("review");
        second.active = false;
        second.pane_index = 1;

        let mut app = app_with_panes(vec![first, second], vec![]);
        app.show_browse_view();
        app.set_status_message_for_test("Showing demo / review #1 in tmux.");

        let lines = render_grid(&app, 104, 18);
        let screen = screen_text(&lines);
        let footer = lines.last().expect("footer should render");

        assert_render_invariants(&lines, 104, 18);
        assert!(screen.contains("Browse"), "{screen}");
        for term in [
            "Showing demo",
            "J/K browse",
            "Enter window",
            "G show",
            "/ filter",
            ". more",
            "Esc back",
            "Q quit",
        ] {
            assert!(
                footer.contains(term),
                "Browse feedback footer should keep `{term}` visible:\n{screen}"
            );
        }
        for inert in ["Space add", ": send"] {
            assert!(
                !footer.contains(inert),
                "Browse feedback footer advertised inert action `{inert}`:\n{screen}"
            );
        }
        assert_no_low_value_copy("browse feedback footer", &lines);
    }

    #[test]
    fn usability_action_contract_browse_enter_window_and_backspace_are_real_footer_actions() {
        let mut first = sample_pane("codex");
        first.id = String::from("%1");
        first.window_id = String::from("@1");
        first.window_name = String::from("alpha");
        first.current_path = String::from("/workspace/alpha");
        let mut second = sample_pane("claude");
        second.id = String::from("%2");
        second.window_id = String::from("@2");
        second.window_name = String::from("beta");
        second.current_path = String::from("/workspace/beta");
        second.active = false;
        second.pane_index = 1;

        let mut app = app_with_panes(
            vec![first, second],
            vec![
                ("%1", vec!["STATUS=running | NEXT=alpha work"]),
                ("%2", vec!["STATUS=waiting | NEXT=beta approval"]),
            ],
        );
        let runtime = tokio::runtime::Runtime::new().expect("runtime should build");

        let render = |app: &App, label: &str| {
            let lines = render_grid(app, 104, 18);
            let screen = screen_text(&lines);
            assert_render_invariants(&lines, 104, 18);
            assert_screen_has_one_line_chrome(label, &lines);
            assert_no_low_value_copy(label, &lines);
            (lines, screen)
        };
        let press = |app: &mut App, key| {
            runtime
                .block_on(handle_key_press(app, key))
                .expect("visible Browse action should run")
        };

        app.show_browse_view();
        let (browse_lines, browse_screen) = render(&app, "browse before enter");
        let browse_footer = browse_lines.last().expect("footer should render");
        for term in ["Browse", "alpha", "beta"] {
            assert!(
                browse_screen.contains(term),
                "Browse should expose window `{term}` before drilling in:\n{browse_screen}"
            );
        }
        for term in ["J/K browse", "Enter window", "Esc back"] {
            assert!(
                browse_footer.contains(term),
                "Browse footer promised `{term}` but did not render it:\n{browse_screen}"
            );
        }

        press(&mut app, KeyCode::Char('j'));
        let (_selected_lines, selected_screen) = render(&app, "browse after movement");
        assert!(
            selected_screen.contains(">  beta") || selected_screen.contains("> beta"),
            "J should visibly move the Browse selection to beta:\n{selected_screen}"
        );

        press(&mut app, KeyCode::Enter);
        let (scoped_lines, scoped_screen) = render(&app, "browse after enter window");
        let scoped_footer = scoped_lines.last().expect("footer should render");
        assert!(
            scoped_screen.contains("1-1 / 1"),
            "Enter window should narrow the Fleet to one pane:\n{scoped_screen}"
        );
        assert!(scoped_screen.contains("beta"), "{scoped_screen}");
        assert!(
            !scoped_screen.contains("alpha"),
            "Enter window must hide the unscoped alpha window:\n{scoped_screen}"
        );
        assert!(
            scoped_footer.contains("backspace show all"),
            "Scoped Browse footer must expose Backspace recovery:\n{scoped_screen}"
        );
        assert!(
            scoped_footer.contains("J/K browse") && scoped_footer.contains("Enter window"),
            "Scoped Browse footer must keep Browse actions real and visible:\n{scoped_screen}"
        );

        press(&mut app, KeyCode::Backspace);
        let (recovered_lines, recovered_screen) = render(&app, "browse after backspace recovery");
        let recovered_footer = recovered_lines.last().expect("footer should render");
        assert!(
            recovered_screen.contains("1-2 / 2"),
            "Backspace should restore all panes from Browse scope:\n{recovered_screen}"
        );
        for term in ["Browse", "alpha", "beta"] {
            assert!(
                recovered_screen.contains(term),
                "Backspace recovery should restore `{term}`:\n{recovered_screen}"
            );
        }
        assert!(
            !recovered_footer.contains("backspace show all"),
            "Recovered Browse footer should not advertise stale narrowing:\n{recovered_screen}"
        );
        assert!(
            recovered_footer.contains("J/K browse") && recovered_footer.contains("Enter window"),
            "Recovered Browse footer should return to normal Browse actions:\n{recovered_screen}"
        );
        let recovered_lines_80 = render_grid(&app, 80, 20);
        let recovered_screen_80 = screen_text(&recovered_lines_80);
        let recovered_footer_80 = recovered_lines_80
            .last()
            .expect("80-column footer should render");
        for term in ["? help", "J/K browse", "Enter window", "Esc back"] {
            assert!(
                recovered_footer_80.contains(term),
                "80-column Browse recovery footer should keep `{term}` visible instead of status-only feedback:\n{recovered_screen_80}"
            );
        }
        assert!(
            !recovered_footer_80.contains("Showing all panes."),
            "80-column Browse recovery footer should spend scarce space on actions, not status-only feedback:\n{recovered_screen_80}"
        );
    }

    #[test]
    fn usability_action_feedback_never_steals_the_footer_keymap_on_roomy_screens() {
        let mut first = sample_pane("bash");
        first.id = String::from("%1");
        let mut second = sample_pane("codex");
        second.id = String::from("%2");
        second.active = false;
        second.pane_index = 1;
        let mut app = app_with_panes(vec![first, second], vec![]);

        app.select_next_pane();
        app.toggle_selected_mark();
        let lines = render_grid(&app, 140, 18);
        let screen = screen_text(&lines);
        let footer = lines.last().expect("footer should render");

        assert_render_invariants(&lines, 140, 18);
        assert!(
            !footer.contains("Added demo / agents #1"),
            "low-value mark feedback should not replace the footer keymap:\n{screen}"
        );
        for term in [
            "? help",
            "J/K move",
            "Space remove",
            "X clear",
            ": send",
            "Q quit",
        ] {
            assert!(
                footer.contains(term),
                "action feedback should not hide `{term}`:\n{screen}"
            );
        }
        assert_no_low_value_copy("action feedback footer", &lines);
    }

    #[test]
    fn usability_settings_actions_show_feedback_without_stranding_navigation() {
        let mut app = app_with_panes(vec![sample_pane("codex")], vec![]);

        app.toggle_desktop_notifications();
        let lines = render_grid(&app, 120, 18);
        let screen = screen_text(&lines);
        let footer = lines.last().expect("footer should render");

        assert_render_invariants(&lines, 120, 18);
        assert!(footer.contains("Desktop alerts"), "{screen}");
        for term in [
            "? help",
            "J/K move",
            "Enter output",
            "Space add",
            ": send",
            "/ filter",
            ". more",
            "Q quit",
        ] {
            assert!(
                footer.contains(term),
                "settings feedback should preserve `{term}`:\n{screen}"
            );
        }
    }

    #[test]
    fn usability_details_focus_never_replaces_navigation_with_status_chatter() {
        let mut app = app_with_panes(
            vec![sample_pane("codex")],
            vec![(
                "%1",
                vec!["STATUS=running | BLOCKER=none | NEXT=write tests"],
            )],
        );
        app.cycle_panel_focus();

        for label in ["output", "send", "browse", "overview", "details"] {
            app.cycle_context_pane();
            app.select_next_pane();
            app.select_previous_pane();
            let lines = render_grid(&app, 120, 18);
            let screen = screen_text(&lines);
            assert_render_invariants(&lines, 120, 18);
            assert_screen_has_one_line_chrome(label, &lines);
            assert!(!screen.contains("Details focused"), "{screen}");
            assert!(
                lines.last().is_some_and(|line| line.contains("? help")),
                "{screen}"
            );
            assert!(
                lines.last().is_some_and(|line| line.contains("J/K move")
                    || line.contains("K older/J newer")
                    || line.contains("J/K browse")),
                "{screen}"
            );
        }
    }

    #[test]
    fn usability_status_feedback_keeps_details_movement_visible() {
        let mut app = app_with_panes(
            vec![sample_pane("codex")],
            vec![(
                "%1",
                vec![
                    "step 01 plan",
                    "step 02 build",
                    "step 03 test",
                    "step 04 polish",
                    "step 05 render",
                    "step 06 inspect",
                    "step 07 patch",
                    "step 08 ux",
                    "step 09 live",
                    "step 10 perf",
                    "step 11 ci",
                    "step 12 review",
                    "step 13 bless",
                    "step 14 audit",
                    "step 15 package",
                    "step 16 smoke",
                    "step 17 final",
                    "step 18 done",
                ],
            )],
        );
        let runtime = tokio::runtime::Runtime::new().expect("runtime should build");

        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Enter))
            .expect("enter should open output");
        app.set_status_message_for_test("Desktop alerts enabled.");
        let output_lines = render_grid(&app, 104, 18);
        let output_screen = screen_text(&output_lines);
        let output_footer = output_lines.last().expect("footer should render");

        assert_render_invariants(&output_lines, 104, 18);
        assert!(app.is_details_panel_focused());
        assert!(output_footer.contains("? help"), "{output_screen}");
        assert!(output_footer.contains("K older/J newer"), "{output_screen}");
        assert!(output_footer.contains("Esc back"), "{output_screen}");
        assert!(
            !output_footer.contains("focused"),
            "status chatter must not replace useful key hints:\n{output_screen}"
        );

        app.show_browse_view();
        app.set_status_message_for_test("Showing demo / agents #1 in tmux.");
        let browse_lines = render_grid(&app, 104, 18);
        let browse_screen = screen_text(&browse_lines);
        let browse_footer = browse_lines.last().expect("footer should render");

        assert_render_invariants(&browse_lines, 104, 18);
        assert!(app.is_details_panel_focused());
        assert!(browse_footer.contains("? help"), "{browse_screen}");
        assert!(browse_footer.contains("J/K browse"), "{browse_screen}");
        assert!(browse_footer.contains("Enter window"), "{browse_screen}");
        assert!(browse_footer.contains("Esc back"), "{browse_screen}");
        assert!(
            !browse_footer.contains("focused"),
            "status chatter must not replace browse key hints:\n{browse_screen}"
        );

        let mut hidden_marked = app_with_panes(vec![sample_pane("codex")], vec![]);
        hidden_marked.toggle_selected_mark();
        hidden_marked.set_search_query_for_test("zz-no-match");
        hidden_marked.set_status_message_for_test("No panes remain for `echo gone`.");
        let hidden_lines = render_grid(&hidden_marked, 104, 18);
        let hidden_screen = screen_text(&hidden_lines);
        let hidden_footer = hidden_lines.last().expect("footer should render");

        assert_render_invariants(&hidden_lines, 104, 18);
        for term in [
            "No panes remain",
            "1 pane hidden",
            ": send",
            "X clear",
            "backspace show all",
        ] {
            assert!(
                hidden_footer.contains(term),
                "hidden send-list footer lost `{term}`:\n{hidden_screen}"
            );
        }
        for inert in ["J/K move", "Enter output", "Space add"] {
            assert!(
                !hidden_footer.contains(inert),
                "hidden send-list footer advertised inert `{inert}`:\n{hidden_screen}"
            );
        }
    }

    #[test]
    fn usability_browse_footer_only_lists_browse_actions_when_navigator_is_focused() {
        let mut first = sample_pane("codex");
        first.id = String::from("%1");
        first.window_id = String::from("@1");
        first.window_name = String::from("alpha");
        let mut second = sample_pane("bash");
        second.id = String::from("%2");
        second.window_id = String::from("@2");
        second.window_name = String::from("shell");
        second.active = false;
        second.pane_index = 1;

        let mut populated = app_with_panes(vec![first, second], vec![]);
        populated.show_browse_view();
        let empty = app_from_panel_fixture(&panel_fixture("navigator_empty_state"));

        let populated_lines = render_grid(&populated, 100, 28);
        assert_render_invariants(&populated_lines, 100, 28);
        assert_screen_has_one_line_chrome("populated browse footer", &populated_lines);
        assert_populated_browse_footer_has_only_browse_actions(
            "populated browse footer",
            &populated_lines,
        );

        let empty_lines = render_grid(&empty, 100, 28);
        assert_render_invariants(&empty_lines, 100, 28);
        assert_screen_has_one_line_chrome("empty browse footer", &empty_lines);
        assert_empty_browse_footer_has_only_recovery_actions("empty browse footer", &empty_lines);
    }

    #[test]
    fn usability_browse_and_command_center_do_not_advertise_fake_focus_switching() {
        let mut first = sample_pane("codex");
        first.id = String::from("%1");
        first.window_id = String::from("@1");
        first.window_name = String::from("agents");
        let mut second = sample_pane("bash");
        second.id = String::from("%2");
        second.window_id = String::from("@2");
        second.window_name = String::from("shell");
        second.active = false;
        second.pane_index = 1;

        let runtime = tokio::runtime::Runtime::new().expect("runtime should build");
        let theme = Theme::from_preset(ThemePreset::Calm);
        let mut app = app_with_panes(vec![first, second], vec![]);

        app.show_browse_view();
        let lines = render_grid(&app, 120, 18);
        let screen = screen_text(&lines);
        let footer = lines.last().expect("footer should render");

        assert_render_invariants(&lines, 120, 18);
        assert!(screen.contains("Browse"), "{screen}");
        assert!(footer.contains("J/K browse"), "{screen}");
        assert!(!footer.contains("Tab focus"), "{screen}");

        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Tab))
            .expect("Tab should be safe in Browse");
        assert!(app.is_details_panel_focused());
        let buffer = render_buffer(&app, 120, 18);
        let lines = buffer_grid_lines(&buffer);
        let screen = screen_text(&lines);
        let footer = lines.last().expect("footer should render");
        let fleet_corner = buffer.cell((0, 1)).expect("fleet border should exist");

        assert_render_invariants(&lines, 120, 18);
        assert!(footer.contains("J/K browse"), "{screen}");
        assert!(!footer.contains("Tab focus"), "{screen}");
        assert_ne!(fleet_corner.fg, theme.accent);

        app.show_command_center();
        let buffer = render_buffer(&app, 120, 18);
        let lines = buffer_grid_lines(&buffer);
        let screen = screen_text(&lines);
        let footer = lines.last().expect("footer should render");
        let fleet_corner = buffer.cell((0, 1)).expect("fleet border should exist");

        assert_render_invariants(&lines, 120, 18);
        assert!(screen.contains("Command Center"), "{screen}");
        assert!(footer.contains("J/K move"), "{screen}");
        assert!(!footer.contains("Tab focus"), "{screen}");
        assert_ne!(fleet_corner.fg, theme.accent);

        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Tab))
            .expect("Tab should be safe in Command Center");
        assert!(app.is_details_panel_focused());
        let lines = render_grid(&app, 120, 18);
        let screen = screen_text(&lines);
        let footer = lines.last().expect("footer should render");

        assert_render_invariants(&lines, 120, 18);
        assert!(screen.contains("Command Center"), "{screen}");
        assert!(!footer.contains("Tab focus"), "{screen}");
    }

    #[test]
    fn usability_command_center_surfaces_stuck_agents_as_needs_you() {
        let pane = sample_pane("claude");
        let mut app = app_with_panes(vec![pane], vec![("%1", vec!["thinking"])]);
        mark_pane_runtime_stale(&mut app, "%1", Duration::from_secs(240));
        app.show_command_center();

        let lines = render_grid(&app, 110, 18);
        let screen = screen_text(&lines);

        assert_render_invariants(&lines, 110, 18);
        assert!(screen.contains("Command Center"), "{screen}");
        assert!(
            screen.contains("Action: Enter output demo / agents"),
            "{screen}"
        );
        assert!(screen.contains("Needs you: 1 stuck"), "{screen}");
        assert!(!screen.contains("Working: none"), "{screen}");
    }

    #[test]
    fn usability_browse_search_keeps_the_visible_window_selected() {
        let mut first = sample_pane("codex");
        first.id = String::from("%1");
        first.window_id = String::from("@1");
        first.window_name = String::from("alpha");

        let mut second = sample_pane("bash");
        second.id = String::from("%2");
        second.window_id = String::from("@2");
        second.window_name = String::from("shell");
        second.active = false;
        second.pane_index = 1;

        let mut app = app_with_panes(vec![first, second], vec![]);
        app.show_browse_view();
        app.select_next_pane();

        app.begin_search();
        for ch in "alpha".chars() {
            app.push_search_char(ch);
        }
        app.finish_search();

        let lines = render_grid(&app, 100, 18);
        let screen = screen_text(&lines);

        assert_render_invariants(&lines, 100, 18);
        assert!(screen.contains("Browse"), "{screen}");
        assert!(screen.contains(">  alpha"), "{screen}");
        assert!(
            !screen.contains(">  shell"),
            "Browse should not keep a hidden window selected:\n{screen}"
        );
        assert_populated_browse_footer_has_only_browse_actions("browse search", &lines);
    }

    #[test]
    fn usability_browse_never_renders_without_a_visible_selection() {
        let mut first = sample_pane("codex");
        first.id = String::from("%1");
        first.window_id = String::from("@1");
        first.window_name = String::from("alpha");

        let mut hidden = sample_pane("bash");
        hidden.id = String::from("%2");
        hidden.window_id = String::from("@2");
        hidden.window_name = String::from("shell");
        hidden.active = false;
        hidden.pane_index = 1;

        let mut app = app_with_panes(vec![first, hidden], vec![]);
        app.show_browse_view();
        app.set_selected_window_id_for_test(Some(String::from("@2")));
        app.set_search_query_for_test("alpha");

        let lines = render_grid(&app, 100, 18);
        let screen = screen_text(&lines);

        assert_render_invariants(&lines, 100, 18);
        assert!(screen.contains("Browse"), "{screen}");
        assert!(screen.contains(">  alpha"), "{screen}");
        assert!(
            !screen.contains(">  shell"),
            "Browse should never render a hidden selection:\n{screen}"
        );
        assert_populated_browse_footer_has_only_browse_actions("stale browse selection", &lines);
    }

    #[test]
    fn usability_view_and_search_navigation_do_not_steal_the_keymap_footer() {
        let mut app = app_with_panes(
            vec![sample_pane("codex")],
            vec![(
                "%1",
                vec!["STATUS=running | BLOCKER=none | NEXT=write tests"],
            )],
        );
        let runtime = tokio::runtime::Runtime::new().expect("runtime should build");

        for label in ["output", "send", "browse", "overview", "details"] {
            app.cycle_context_pane();
            let lines = render_grid(&app, 120, 18);
            let screen = screen_text(&lines);
            assert_render_invariants(&lines, 120, 18);
            if label == "browse" {
                assert_populated_browse_footer_has_only_browse_actions(label, &lines);
            } else if label == "overview" {
                let footer = lines.last().expect("footer should render");
                assert!(footer.contains("Enter output"), "{screen}");
                assert!(
                    !footer.contains(": send"),
                    "all-clear Command Center should not make send look like the next step:\n{screen}"
                );
            } else {
                assert_footer_keeps_core_keys(label, &lines);
            }
            assert!(!screen.contains("Secondary view switched"), "{screen}");
        }

        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Char('/')))
            .expect("search should open");
        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Char('z')))
            .expect("search typing should work");
        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Backspace))
            .expect("search backspace should work");
        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Enter))
            .expect("search clear should apply");

        let lines = render_grid(&app, 120, 18);
        let screen = screen_text(&lines);
        assert_render_invariants(&lines, 120, 18);
        assert_footer_keeps_core_keys("search clear", &lines);
        assert!(!screen.contains("Cleared pane search"), "{screen}");

        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Char('/')))
            .expect("search should open");
        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Esc))
            .expect("search cancel should close");

        let lines = render_grid(&app, 120, 18);
        let screen = screen_text(&lines);
        assert_render_invariants(&lines, 120, 18);
        assert_footer_keeps_core_keys("search cancel", &lines);
        assert!(!screen.contains("Closed pane search input"), "{screen}");
    }

    #[test]
    fn usability_empty_tmux_state_is_actionable_not_jargon() {
        let app = app_with_panes(Vec::new(), vec![]);

        for &(width, height) in &[(80, 16), (120, 18)] {
            let lines = render_grid(&app, width, height);
            let screen = screen_text(&lines);
            assert_render_invariants(&lines, width, height);
            assert_screen_has_one_line_chrome("empty tmux", &lines);
            assert!(screen.contains("No panes yet."), "{screen}");
            assert!(
                screen.contains("Start tmux panes, then R refresh."),
                "{screen}"
            );
            assert!(!screen.contains("No panes in view."), "{screen}");
            assert!(
                !screen.contains("Clear scope or wait for tmux."),
                "{screen}"
            );
        }

        let mut unreadable = app_with_panes(Vec::new(), vec![]);
        unreadable.set_status_message_for_test(
            "Could not read tmux panes for socket `agents`, session `ops`: permission denied.",
        );

        for &(width, height) in &[(80, 16), (120, 18)] {
            let lines = render_grid(&unreadable, width, height);
            let screen = screen_text(&lines);
            assert_render_invariants(&lines, width, height);
            assert_screen_has_one_line_chrome("unreadable tmux", &lines);
            assert!(screen.contains("Cannot read tmux panes."), "{screen}");
            assert!(
                screen.contains("Check socket/session, then R refresh."),
                "{screen}"
            );
            assert!(!screen.contains("Could not read tmux panes"), "{screen}");
            assert!(!screen.contains("permission denied"), "{screen}");
        }
    }

    #[test]
    fn tiny_empty_tmux_recovery_keeps_refresh_visible_without_fake_actions() {
        let no_panes = app_with_panes(Vec::new(), vec![]);
        let mut no_server = app_with_panes(Vec::new(), vec![]);
        no_server.set_status_message_for_test("No tmux server found for socket `ops`. Start tmux.");
        let mut missing_session = app_with_panes(Vec::new(), vec![]);
        missing_session.set_status_message_for_test("Session not found for session `agents`.");
        let mut unreadable = app_with_panes(Vec::new(), vec![]);
        unreadable.set_status_message_for_test(
            "Could not read tmux panes for socket `ops`: permission denied.",
        );

        let scenarios = vec![
            (
                "no panes",
                no_panes,
                ["No panes yet.", "Start tmux panes, then R refresh."],
            ),
            (
                "no server",
                no_server,
                ["No tmux server.", "Start tmux, then R refresh."],
            ),
            (
                "missing session",
                missing_session,
                ["Session not found.", "Use another session, then R refresh."],
            ),
            (
                "unreadable tmux",
                unreadable,
                [
                    "Cannot read tmux panes.",
                    "Check socket/session, then R refresh.",
                ],
            ),
        ];

        for (name, app, recovery_terms) in scenarios {
            let lines = render_grid(&app, 60, 12);
            let screen = screen_text(&lines);
            let footer = lines.last().expect("footer should render");

            assert_render_invariants(&lines, 60, 12);
            assert_screen_has_one_line_chrome(name, &lines);
            for term in recovery_terms {
                assert!(screen.contains(term), "{name} missing `{term}`:\n{screen}");
            }
            for term in ["? help", "R refresh", ". more", "Q quit"] {
                assert!(
                    footer.contains(term),
                    "{name} footer should keep recovery action `{term}` visible:\n{screen}"
                );
            }
            for inert in ["J/K move", "Enter output", ": send", "/ filter", "G show"] {
                assert!(
                    !screen.contains(inert),
                    "{name} advertised inert `{inert}` with no panes:\n{screen}"
                );
            }
        }
    }

    #[test]
    fn tiny_empty_tmux_secondary_views_keep_refresh_and_back_visible() {
        let mut output = app_with_panes(Vec::new(), vec![]);
        output.cycle_context_pane();

        let mut browse = app_with_panes(Vec::new(), vec![]);
        browse.show_browse_view();

        let mut overview = app_with_panes(Vec::new(), vec![]);
        overview.show_command_center();

        for (name, app, title) in [
            ("output", output, "Output"),
            ("browse", browse, "Browse"),
            ("command center", overview, "Command Center"),
        ] {
            let lines = render_grid(&app, 60, 12);
            let screen = screen_text(&lines);
            let footer = lines.last().expect("footer should render");

            assert_render_invariants(&lines, 60, 12);
            assert_screen_has_one_line_chrome(name, &lines);
            assert!(
                screen.contains(title),
                "{name} missing `{title}`:\n{screen}"
            );
            for term in ["? help", "R refresh", "Esc back", ". more", "Q quit"] {
                assert!(
                    footer.contains(term),
                    "{name} footer should keep recovery/back action `{term}` visible:\n{screen}"
                );
            }
            for inert in ["J/K move", "Enter output", ": send", "/ filter", "G show"] {
                assert!(
                    !screen.contains(inert),
                    "{name} advertised inert `{inert}` with no panes:\n{screen}"
                );
            }
        }
    }

    #[test]
    fn usability_details_spends_short_screen_space_on_output_before_metadata() {
        let pane = sample_pane("bash");
        let app = app_with_panes(
            vec![pane],
            vec![(
                "%1",
                vec![
                    "marker01 prepare cache",
                    "marker02 fetch dependencies",
                    "marker03 compile crate",
                    "marker04 run unit tests",
                    "marker05 package binary",
                    "marker06 upload artifact",
                    "marker07 notify operator",
                    "marker08 verify release",
                    "marker09 done",
                ],
            )],
        );

        let lines = render_grid(&app, 100, 14);
        let screen = screen_text(&lines);
        let marker_count = screen.matches("marker").count();

        assert_render_invariants(&lines, 100, 14);
        assert!(screen.contains("Details"), "{screen}");
        assert!(screen.contains("Output"), "{screen}");
        assert!(
            marker_count >= 4,
            "short Details should preserve useful output before lower-value metadata; saw {marker_count}\n{screen}"
        );
        assert!(
            !screen.contains("Updated:"),
            "short Details should not spend scarce rows on metadata before output:\n{screen}"
        );
    }

    #[test]
    fn usability_output_view_uses_available_space_for_latest_tail() {
        let pane = sample_pane("bash");
        let mut app = app_with_panes(
            vec![pane],
            vec![(
                "%1",
                vec![
                    "marker01 prepare cache",
                    "marker02 fetch dependencies",
                    "marker03 compile crate",
                    "marker04 run unit tests",
                    "marker05 package binary",
                    "marker06 upload artifact",
                    "marker07 notify operator",
                    "marker08 verify release",
                ],
            )],
        );
        app.cycle_context_pane();
        app.close_help_overlay();

        let lines = render_grid(&app, 100, 16);
        let screen = screen_text(&lines);
        let marker_count = screen.matches("marker").count();

        assert_render_invariants(&lines, 100, 16);
        assert!(screen.contains("Output"), "{screen}");
        assert!(screen.contains("Latest"), "{screen}");
        assert!(
            marker_count >= 4,
            "Output view should use available space for recent useful tail; saw {marker_count}\n{screen}"
        );
    }

    #[test]
    fn scrollbar_geometry_uses_viewport_ratio_and_exact_endpoints() {
        assert_eq!(ScrollbarGeometry::new(10, 10, 0, 10), None);
        assert_eq!(ScrollbarGeometry::new(11, 10, 0, 1), None);

        assert_eq!(
            ScrollbarGeometry::new(11, 10, 0, 10),
            Some(ScrollbarGeometry {
                track_len: 10,
                thumb_start: 0,
                thumb_len: 9,
            })
        );
        assert_eq!(
            ScrollbarGeometry::new(11, 10, 1, 10),
            Some(ScrollbarGeometry {
                track_len: 10,
                thumb_start: 1,
                thumb_len: 9,
            })
        );
        assert_eq!(
            ScrollbarGeometry::new(100, 10, 0, 20),
            Some(ScrollbarGeometry {
                track_len: 20,
                thumb_start: 0,
                thumb_len: 2,
            })
        );
        assert_eq!(
            ScrollbarGeometry::new(100, 10, 45, 20),
            Some(ScrollbarGeometry {
                track_len: 20,
                thumb_start: 9,
                thumb_len: 2,
            })
        );
        assert_eq!(
            ScrollbarGeometry::new(100, 10, 90, 20),
            Some(ScrollbarGeometry {
                track_len: 20,
                thumb_start: 18,
                thumb_len: 2,
            })
        );
        assert_eq!(
            ScrollbarGeometry::new(100, 10, 9_999, 20),
            ScrollbarGeometry::new(100, 10, 90, 20)
        );
    }

    #[test]
    fn usability_output_height_scales_across_required_terminal_sizes() {
        let mut counts = Vec::new();

        for &(width, height, minimum_visible) in &[
            (80, 24, 12),
            (100, 24, 12),
            (120, 30, 18),
            (132, 36, 24),
            (100, 14, 4),
        ] {
            let mut app = app_with_numbered_output("roomy", 50);
            app.cycle_context_pane();
            let lines = render_grid(&app, width, height);
            let screen = screen_text(&lines);
            let visible = count_output_rows(&lines, "roomy output");

            assert_render_invariants(&lines, width, height);
            assert!(screen.contains("Output"), "{width}x{height}\n{screen}");
            assert!(screen.contains("Latest"), "{width}x{height}\n{screen}");
            assert!(
                visible >= minimum_visible,
                "Output should use available height at {width}x{height}; saw {visible}\n{screen}"
            );
            counts.push((width, height, visible));
        }

        let short_visible = counts
            .iter()
            .find(|(width, height, _)| *width == 100 && *height == 14)
            .map(|(_, _, visible)| *visible)
            .expect("short terminal count should be recorded");
        let tall_visible = counts
            .iter()
            .find(|(width, height, _)| *width == 132 && *height == 36)
            .map(|(_, _, visible)| *visible)
            .expect("tall terminal count should be recorded");
        assert!(
            tall_visible > short_visible + 12,
            "taller terminals should buy visibly more Output rows: {counts:?}"
        );

        let details = app_with_numbered_output("stacked", 40);
        let lines = render_grid(&details, 80, 24);
        let screen = screen_text(&lines);
        assert_render_invariants(&lines, 80, 24);
        assert!(
            line_index(&lines, "Details") > line_index(&lines, "Fleet"),
            "80x24 should use the narrow stacked layout instead of cramped columns:\n{screen}"
        );
        assert!(
            count_output_rows(&lines, "stacked output") >= 5,
            "narrow stacked Details should still preserve useful output:\n{screen}"
        );
    }

    #[test]
    fn usability_dock_sized_layouts_spend_height_on_details_and_output() {
        for &(width, minimum_details_rows, minimum_output_rows) in
            &[(44, 10, 22), (52, 12, 24), (72, 14, 26)]
        {
            let details = app_with_numbered_output("dock details", 60);
            let lines = render_grid(&details, width, 40);
            let screen = screen_text(&lines);

            assert_render_invariants(&lines, width, 40);
            assert!(screen.contains("Fleet"), "{width}x40\n{screen}");
            assert!(screen.contains("Details"), "{width}x40\n{screen}");
            assert!(
                line_index(&lines, "Details") > line_index(&lines, "Fleet"),
                "dock-sized terminals should stack Fleet above Details:\n{screen}"
            );
            assert!(
                count_output_rows(&lines, "dock details") >= minimum_details_rows,
                "Details should use dock height at {width}x40:\n{screen}"
            );

            let mut output = app_with_numbered_output("dock output", 80);
            output.cycle_context_pane();
            let lines = render_grid(&output, width, 40);
            let screen = screen_text(&lines);

            assert_render_invariants(&lines, width, 40);
            assert!(screen.contains("Output"), "{width}x40\n{screen}");
            assert!(
                count_output_rows(&lines, "dock output") >= minimum_output_rows,
                "Output should use dock height at {width}x40:\n{screen}"
            );
        }
    }

    #[test]
    fn usability_scrollbar_is_spatial_focused_and_not_inert() {
        let runtime = tokio::runtime::Runtime::new().expect("runtime should build");
        let mut bottom = app_with_numbered_output("scrollbar", 50);
        runtime
            .block_on(handle_key_press(&mut bottom, KeyCode::Enter))
            .expect("enter should open output");
        let bottom_lines = render_grid(&bottom, 100, 24);
        let bottom_screen = screen_text(&bottom_lines);
        let bottom_thumb = scrollbar_thumb_center_row(&bottom_lines);

        assert_render_invariants(&bottom_lines, 100, 24);
        assert!(
            bottom_screen.contains("scrollbar output 50"),
            "{bottom_screen}"
        );
        assert!(
            !bottom_screen.contains("scrolled"),
            "the scrollbar should carry position, not status chatter:\n{bottom_screen}"
        );
        assert!(
            !bottom_lines[line_index(&bottom_lines, "demo / agents")].contains('░')
                && !bottom_lines[line_index(&bottom_lines, "demo / agents")].contains('█')
                && !bottom_lines[line_index(&bottom_lines, "Running")].contains('░')
                && !bottom_lines[line_index(&bottom_lines, "Running")].contains('█')
                && !bottom_lines[line_index(&bottom_lines, "Latest")].contains('░')
                && !bottom_lines[line_index(&bottom_lines, "Latest")].contains('█'),
            "scrollbar track should align with the scrollable output rows, not panel metadata:\n{bottom_screen}"
        );
        assert_scrollbar_matches_output_rows(&bottom_lines, "scrollbar output", 50);

        let mut middle = app_with_numbered_output("scrollbar", 50);
        runtime
            .block_on(handle_key_press(&mut middle, KeyCode::Enter))
            .expect("enter should open output");
        for _ in 0..10 {
            runtime
                .block_on(handle_key_press(&mut middle, KeyCode::Char('k')))
                .expect("K should scroll older");
        }
        let middle_lines = render_grid(&middle, 100, 24);
        let middle_thumb = scrollbar_thumb_center_row(&middle_lines);
        assert_scrollbar_matches_output_rows(&middle_lines, "scrollbar output", 50);

        let mut top = app_with_numbered_output("scrollbar", 50);
        runtime
            .block_on(handle_key_press(&mut top, KeyCode::Enter))
            .expect("enter should open output");
        for _ in 0..100 {
            runtime
                .block_on(handle_key_press(&mut top, KeyCode::Char('k')))
                .expect("K should scroll older");
        }
        let top_lines = render_grid(&top, 100, 24);
        let top_thumb = scrollbar_thumb_center_row(&top_lines);
        assert_scrollbar_matches_output_rows(&top_lines, "scrollbar output", 50);

        assert!(
            top_thumb < middle_thumb && middle_thumb < bottom_thumb,
            "scrollbar thumb should move spatially from top to middle to bottom: top={top_thumb}, middle={middle_thumb}, bottom={bottom_thumb}"
        );
        assert_eq!(
            panel_border_signature(&bottom_lines),
            panel_border_signature(&middle_lines),
            "scrollbar should not move Output chrome"
        );
        assert_eq!(
            panel_border_signature(&bottom_lines),
            panel_border_signature(&top_lines),
            "scrollbar should not move Output chrome at extremes"
        );

        let mut non_scrollable = app_with_panes(
            vec![sample_pane("bash")],
            vec![("%1", vec!["one short line", "two short lines"])],
        );
        runtime
            .block_on(handle_key_press(&mut non_scrollable, KeyCode::Enter))
            .expect("enter should open output");
        let non_scrollable_lines = render_grid(&non_scrollable, 100, 24);
        assert_render_invariants(&non_scrollable_lines, 100, 24);
        assert_no_scrollbar(&non_scrollable_lines);

        let fleet_focus = app_with_numbered_output("focus", 50);
        let fleet_lines = render_grid(&fleet_focus, 100, 24);
        assert_render_invariants(&fleet_lines, 100, 24);
        assert_no_scrollbar(&fleet_lines);

        let mut details_focus = app_with_numbered_output("focus", 50);
        details_focus.cycle_panel_focus();
        let details_lines = render_grid(&details_focus, 100, 24);
        assert_render_invariants(&details_lines, 100, 24);
        assert!(
            scrollbar_thumb_center_row(&details_lines) >= 0.0,
            "focused scrollable Details should expose a spatial scrollbar"
        );
    }

    #[test]
    fn usability_scrollbar_uses_wrapped_rendered_content_and_reaches_extremes() {
        let output = (1..=8)
            .map(|index| {
                format!(
                    "wrap output {index:02} this is a deliberately long agent update that must wrap into multiple rendered rows before scrolling"
                )
            })
            .collect::<Vec<_>>();
        let output_refs = output.iter().map(String::as_str).collect::<Vec<_>>();
        let mut app = app_with_panes(vec![sample_pane("bash")], vec![("%1", output_refs)]);
        let runtime = tokio::runtime::Runtime::new().expect("runtime should build");

        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Enter))
            .expect("enter should open output");
        let bottom = render_grid(&app, 82, 16);
        let bottom_screen = screen_text(&bottom);
        assert_render_invariants(&bottom, 82, 16);
        assert!(
            bottom_screen.contains("wrap output 08"),
            "newest wrapped output should be visible before scrolling:\n{bottom_screen}"
        );
        assert_scrollbar_thumb_at_bottom(&bottom);

        for _ in 0..80 {
            runtime
                .block_on(handle_key_press(&mut app, KeyCode::Char('k')))
                .expect("K should scroll older through wrapped rows");
        }
        let top = render_grid(&app, 82, 16);
        let top_screen = screen_text(&top);
        assert_render_invariants(&top, 82, 16);
        assert!(
            top_screen.contains("wrap output 01"),
            "scrolling older should reach the true first wrapped output line:\n{top_screen}"
        );
        assert_scrollbar_thumb_at_top(&top);

        for _ in 0..80 {
            runtime
                .block_on(handle_key_press(&mut app, KeyCode::Char('j')))
                .expect("J should scroll newer through wrapped rows");
        }
        let recovered = render_grid(&app, 82, 16);
        assert_eq!(
            normalize_relative_ages(recovered),
            normalize_relative_ages(bottom),
            "scrolling newer should recover the exact newest wrapped viewport"
        );
    }

    #[test]
    fn usability_scrollbar_geometry_survives_focus_resize_and_short_layouts() {
        let runtime = tokio::runtime::Runtime::new().expect("runtime should build");

        for (width, height) in [(82, 24), (100, 14), (132, 30)] {
            let mut output_app = app_with_numbered_output("resize", 60);
            runtime
                .block_on(handle_key_press(&mut output_app, KeyCode::Enter))
                .expect("enter should open output");
            let bottom = render_grid(&output_app, width, height);
            assert_render_invariants(&bottom, width, height);
            assert_scrollbar_matches_output_rows(&bottom, "resize output", 60);
            assert_scrollbar_thumb_at_bottom(&bottom);

            for _ in 0..120 {
                runtime
                    .block_on(handle_key_press(&mut output_app, KeyCode::Char('k')))
                    .expect("K should scroll older");
            }
            let top = render_grid(&output_app, width, height);
            assert_render_invariants(&top, width, height);
            assert_scrollbar_matches_output_rows(&top, "resize output", 60);
            assert_scrollbar_thumb_at_top(&top);

            let mut details_app = app_with_numbered_output("resize", 60);
            details_app.cycle_panel_focus();
            let details = render_grid(&details_app, width, height);
            assert_render_invariants(&details, width, height);
            assert_scrollbar_thumb_at_bottom(&details);
            assert_eq!(
                panel_border_signature(&bottom),
                panel_border_signature(&top),
                "scrollbar movement should not reflow chrome at {width}x{height}"
            );
        }
    }

    #[test]
    fn usability_generic_agent_labels_are_plain_and_do_not_pollute_latest() {
        let pane = sample_pane("node");
        let app = app_with_panes(
            vec![pane],
            vec![("%1", vec!["STATUS=running | BLOCKER=none | NEXT=ship fix"])],
        );

        let lines = render_grid(&app, 100, 16);
        let screen = screen_text(&lines);

        assert_render_invariants(&lines, 100, 16);
        assert!(screen.contains("Tool: Agent"), "{screen}");
        assert!(screen.contains("ship fix"), "{screen}");
        assert!(
            !screen.contains("Generic agent"),
            "generic qualifier is implementation copy, not user value:\n{screen}"
        );
        assert!(
            !screen.contains("agent: ship fix"),
            "Fleet Latest should spend words on the action, not generic tool labels:\n{screen}"
        );
    }

    #[test]
    fn usability_nonstandard_structured_agent_wrappers_render_as_agents_not_jobs() {
        let pane = sample_pane("runner");
        let app = app_with_panes(
            vec![pane],
            vec![(
                "%1",
                vec!["STATUS=waiting | BLOCKER=approval | NEXT=approve"],
            )],
        );

        let lines = render_grid(&app, 100, 16);
        let screen = screen_text(&lines);

        assert_render_invariants(&lines, 100, 16);
        assert!(screen.contains("Tool: Agent"), "{screen}");
        assert!(screen.contains("State: Waiting"), "{screen}");
        assert!(screen.contains("Action: : reply"), "{screen}");
        assert!(!screen.contains("Tool: Job"), "{screen}");
    }

    #[test]
    fn split_label_value_extracts_inspector_labels() {
        assert_eq!(
            split_label_value("Action: press G to open"),
            Some((String::from("Action: "), String::from("press G to open")))
        );
    }

    #[test]
    fn section_headings_are_detected_for_panel_rendering() {
        assert!(is_section_heading("Output"));
        assert!(is_section_heading("Queue"));
        assert!(is_section_heading("Queue (8)"));
        assert!(!is_section_heading("Queue (many)"));
        assert!(!is_section_heading("Waiting  Shell"));
    }

    #[test]
    fn mono_theme_avoids_colored_accents() {
        let theme = Theme::from_preset(ThemePreset::Mono);
        assert_eq!(theme.accent, Color::White);
        assert_eq!(theme.success, Color::White);
        assert_eq!(theme.warning, Color::White);
        assert_eq!(theme.danger, Color::White);
        assert_eq!(theme.text, Color::White);
    }

    #[test]
    fn terminal_native_theme_uses_ansi_colors_to_follow_the_terminal_palette() {
        let theme = Theme::from_preset(ThemePreset::TerminalNative);

        assert_eq!(theme.text, Color::Reset);
        assert_eq!(theme.muted, Color::DarkGray);
        assert_eq!(theme.accent, Color::Blue);
        assert_eq!(theme.success, Color::Green);
        assert_eq!(theme.warning, Color::Yellow);
        assert_eq!(theme.danger, Color::Red);
        assert_eq!(theme.selected_fg, Color::Reset);
        assert_eq!(theme.selected_bg, Color::Reset);
        assert_eq!(theme.selected_row_style().fg, None);
        assert_eq!(theme.selected_row_style().bg, None);
        assert!(
            theme
                .selected_row_style()
                .add_modifier
                .contains(Modifier::REVERSED)
        );
    }

    #[test]
    fn default_theme_is_terminal_native_and_does_not_paint_broad_backgrounds() {
        let settings = UiSettings::default();
        assert_eq!(settings.active_theme_preset(), ThemePreset::TerminalNative);

        let theme = Theme::from_settings(&settings);
        assert_eq!(theme.text, Color::Reset);
        assert_eq!(theme.selected_fg, Color::Reset);
        assert_eq!(theme.selected_bg, Color::Reset);

        let selected = theme.selected_row_style();
        assert_eq!(selected.fg, None);
        assert_eq!(selected.bg, None);
        assert!(selected.add_modifier.contains(Modifier::BOLD));
        assert!(selected.add_modifier.contains(Modifier::REVERSED));
    }

    #[test]
    fn calm_theme_keeps_body_text_native_to_survive_light_and_dark_terminals() {
        let theme = Theme::from_preset(ThemePreset::Calm);

        assert_eq!(theme.text, Color::Reset);
        assert_eq!(theme.accent, Color::LightBlue);
        assert_eq!(theme.selected_fg, Color::White);
        assert_eq!(theme.selected_bg, Color::DarkGray);
    }

    #[test]
    fn theme_overrides_apply_to_semantic_slots_without_widget_specific_config() {
        let settings = UiSettings {
            theme_preset: ThemePreset::Contrast,
            theme: ThemeConfig {
                preset: Some(ThemePreset::CatppuccinLatte),
                overrides: ThemeOverrides {
                    text: Some(ThemeColor::Rgb(56, 58, 66)),
                    muted: Some(ThemeColor::DarkGray),
                    accent: Some(ThemeColor::Rgb(64, 120, 242)),
                    success: Some(ThemeColor::Green),
                    warning: Some(ThemeColor::Indexed(178)),
                    danger: Some(ThemeColor::LightRed),
                    surface: Some(ThemeColor::Rgb(218, 218, 219)),
                    border: Some(ThemeColor::Rgb(162, 162, 163)),
                    selected_fg: Some(ThemeColor::Black),
                    selected_bg: Some(ThemeColor::Rgb(218, 218, 219)),
                },
            },
            ..UiSettings::default()
        };

        let theme = Theme::from_settings(&settings);

        assert_eq!(theme.text, Color::Rgb(56, 58, 66));
        assert_eq!(theme.muted, Color::DarkGray);
        assert_eq!(theme.accent, Color::Rgb(64, 120, 242));
        assert_eq!(theme.success, Color::Green);
        assert_eq!(theme.warning, Color::Indexed(178));
        assert_eq!(theme.danger, Color::LightRed);
        assert_eq!(theme.surface, Color::Rgb(218, 218, 219));
        assert_eq!(theme.border, Color::Rgb(162, 162, 163));
        assert_eq!(theme.selected_row_style().fg, Some(Color::Black));
        assert_eq!(
            theme.selected_row_style().bg,
            Some(Color::Rgb(218, 218, 219))
        );
    }

    #[test]
    fn theme_no_color_and_dumb_profiles_keep_shape_cues_without_color_dependency() {
        let mut env = HashMap::new();
        env.insert(String::from("NO_COLOR"), String::from("1"));
        let profile = TerminalProfile::from_env_map(&env);
        let theme = Theme::from_preset_with_profile(ThemePreset::CatppuccinMocha, profile);
        let selected = theme.selected_row_style();
        let alert = theme.alert_row_style();

        assert_eq!(selected.fg, None);
        assert_eq!(selected.bg, None);
        assert!(selected.add_modifier.contains(Modifier::REVERSED));
        assert!(alert.add_modifier.contains(Modifier::REVERSED));

        let mut dumb_env = HashMap::new();
        dumb_env.insert(String::from("TERM"), String::from("dumb"));
        let dumb_profile = TerminalProfile::from_env_map(&dumb_env);
        let dumb_theme = Theme::from_preset_with_profile(ThemePreset::TerminalNative, dumb_profile);

        assert!(dumb_theme.ascii_borders);
        assert!(!dumb_theme.color);
        assert!(
            dumb_theme
                .targeted_row_style()
                .add_modifier
                .contains(Modifier::UNDERLINED)
        );
    }

    #[test]
    fn usability_theme_presets_keep_selection_alerts_and_targets_distinguishable() {
        for preset in ALL_THEME_PRESETS {
            let theme = Theme::from_preset(preset);
            let selected = theme.selected_row_style();
            let alert = theme.alert_row_style();
            let targeted = theme.targeted_row_style();

            if preset == ThemePreset::TerminalNative {
                assert_eq!(selected.fg, None);
                assert_eq!(selected.bg, None);
                assert!(
                    selected.add_modifier.contains(Modifier::REVERSED),
                    "terminal-native selected rows should use native reverse video"
                );
            } else {
                assert_ne!(
                    selected.fg, selected.bg,
                    "{preset:?} selected rows must not become invisible"
                );
            }
            assert!(
                selected.add_modifier.contains(Modifier::BOLD)
                    || selected.add_modifier.contains(Modifier::REVERSED),
                "{preset:?} selected rows need a non-color cue"
            );
            assert!(
                alert.add_modifier.contains(Modifier::BOLD)
                    || alert.add_modifier.contains(Modifier::REVERSED),
                "{preset:?} alert rows need a non-color cue"
            );
            assert!(
                targeted.add_modifier.contains(Modifier::BOLD)
                    || targeted.add_modifier.contains(Modifier::UNDERLINED),
                "{preset:?} send-list rows need a non-color cue"
            );

            if preset == ThemePreset::Mono {
                assert!(
                    alert.add_modifier.contains(Modifier::REVERSED),
                    "mono alerts need shape, not color"
                );
                assert!(
                    targeted.add_modifier.contains(Modifier::UNDERLINED),
                    "mono send-list rows need shape, not color"
                );
            }
        }
    }

    #[test]
    fn usability_theme_truecolor_presets_keep_selection_contrast() {
        for preset in [
            ThemePreset::CatppuccinLatte,
            ThemePreset::CatppuccinMocha,
            ThemePreset::TokyoNight,
            ThemePreset::GruvboxDark,
            ThemePreset::GruvboxLight,
            ThemePreset::Nord,
            ThemePreset::RosePine,
        ] {
            let theme = Theme::from_preset(preset);
            let ratio = contrast_ratio(
                rgb_tuple(theme.selected_fg).expect("selected fg should be RGB"),
                rgb_tuple(theme.selected_bg).expect("selected bg should be RGB"),
            );

            assert!(
                ratio >= 3.0,
                "{preset:?} selected contrast should be at least 3:1, got {ratio:.2}"
            );
        }
    }

    #[test]
    fn theme_named_truecolor_presets_use_documented_palette_tokens() {
        for (
            preset,
            text,
            muted,
            accent,
            success,
            warning,
            danger,
            border,
            surface,
            selected_fg,
            selected_bg,
        ) in [
            (
                ThemePreset::CatppuccinLatte,
                0x4C4F69,
                0x6C6F85,
                0x1E66F5,
                0x40A02B,
                0xDF8E1D,
                0xD20F39,
                0xACB0BE,
                0xCCD0DA,
                0x4C4F69,
                0xCCD0DA,
            ),
            (
                ThemePreset::CatppuccinMocha,
                0xCDD6F4,
                0x7F849C,
                0x89B4FA,
                0xA6E3A1,
                0xF9E2AF,
                0xF38BA8,
                0x585B70,
                0x313244,
                0xCDD6F4,
                0x45475A,
            ),
            (
                ThemePreset::TokyoNight,
                0xC0CAF5,
                0x565F89,
                0x7AA2F7,
                0x9ECE6A,
                0xE0AF68,
                0xF7768E,
                0x3B4261,
                0x292E42,
                0xC0CAF5,
                0x3B4261,
            ),
            (
                ThemePreset::GruvboxDark,
                0xEBDBB2,
                0x928374,
                0x83A598,
                0xB8BB26,
                0xFABD2F,
                0xFB4934,
                0x665C54,
                0x3C3836,
                0xEBDBB2,
                0x504945,
            ),
            (
                ThemePreset::GruvboxLight,
                0x3C3836,
                0x928374,
                0x076678,
                0x79740E,
                0xB57614,
                0x9D0006,
                0xD5C4A1,
                0xEBDBB2,
                0x3C3836,
                0xD5C4A1,
            ),
            (
                ThemePreset::Nord,
                0xD8DEE9,
                0x4C566A,
                0x88C0D0,
                0xA3BE8C,
                0xEBCB8B,
                0xBF616A,
                0x4C566A,
                0x3B4252,
                0xECEFF4,
                0x434C5E,
            ),
            (
                ThemePreset::RosePine,
                0xE0DEF4,
                0x6E6A86,
                0x9CCFD8,
                0x31748F,
                0xF6C177,
                0xEB6F92,
                0x403D52,
                0x26233A,
                0xE0DEF4,
                0x403D52,
            ),
        ] {
            let theme = Theme::from_preset(preset);
            for (slot, actual, expected) in [
                ("text", theme.text, text),
                ("muted", theme.muted, muted),
                ("accent", theme.accent, accent),
                ("success", theme.success, success),
                ("warning", theme.warning, warning),
                ("danger", theme.danger, danger),
                ("border", theme.border, border),
                ("surface", theme.surface, surface),
                ("selected_fg", theme.selected_fg, selected_fg),
                ("selected_bg", theme.selected_bg, selected_bg),
            ] {
                assert_eq!(
                    rgb_tuple(actual),
                    Some(rgb_hex(expected)),
                    "{preset:?} {slot} should keep its documented palette token"
                );
            }
        }
    }

    #[test]
    fn usability_theme_presets_render_core_states_at_cell_level() {
        let runtime = tokio::runtime::Runtime::new().expect("runtime should build");
        for preset in ALL_THEME_PRESETS {
            let fixture = panel_fixture("selected_waiting_panel");
            let mut selected_app = app_from_panel_fixture(&fixture);
            selected_app.set_theme_for_test(ThemeConfig {
                preset: Some(preset),
                overrides: ThemeOverrides::default(),
            });
            let selected_theme = Theme::from_settings(selected_app.ui_settings());
            let selected_buffer = render_buffer(&selected_app, 120, 18);
            let selected_lines = buffer_grid_lines(&selected_buffer);

            let focused_border = selected_buffer
                .cell((0, 1))
                .expect("fleet border should exist");
            assert_eq!(focused_border.fg, selected_theme.accent, "{preset:?}");

            let selected_y = selected_lines
                .iter()
                .position(|line| line.contains(">! demo/agents"))
                .expect("selected fleet row should render");
            let selected = buffer_cell_in_line(
                &selected_buffer,
                &selected_lines[selected_y],
                selected_y,
                "demo/agents",
            );
            assert_eq!(selected.fg, selected_theme.selected_fg, "{preset:?}");
            assert_eq!(selected.bg, selected_theme.selected_bg, "{preset:?}");
            assert!(selected.modifier.contains(Modifier::BOLD), "{preset:?}");
            let selected_marker = buffer_cell_in_line(
                &selected_buffer,
                &selected_lines[selected_y],
                selected_y,
                "!",
            );
            assert_eq!(selected_marker.fg, selected_theme.warning, "{preset:?}");
            assert_eq!(
                selected_marker.bg, selected_theme.selected_bg,
                "{preset:?} selected attention marker should keep the selected row background"
            );
            assert!(
                selected_marker.modifier.contains(Modifier::BOLD),
                "{preset:?}"
            );

            let selected_latest = buffer_cell_in_line(
                &selected_buffer,
                &selected_lines[selected_y],
                selected_y,
                "needs you",
            );
            assert_eq!(selected_latest.fg, selected_theme.warning, "{preset:?}");
            assert_eq!(
                selected_latest.bg, selected_theme.selected_bg,
                "{preset:?} selected attention latest should keep the selected row background"
            );

            let mut alert_first = sample_pane("bash");
            alert_first.id = String::from("%1");
            alert_first.window_name = String::from("idle");

            let mut alert_second = sample_pane("bash");
            alert_second.id = String::from("%2");
            alert_second.window_name = String::from("alert");
            alert_second.active = false;
            alert_second.pane_index = 1;

            let mut alert_app = app_with_panes(
                vec![alert_first, alert_second],
                vec![("%1", vec!["idle"]), ("%2", vec!["error: command failed"])],
            );
            alert_app.set_theme_for_test(ThemeConfig {
                preset: Some(preset),
                overrides: ThemeOverrides::default(),
            });
            let theme = Theme::from_settings(alert_app.ui_settings());
            let buffer = render_buffer(&alert_app, 120, 18);
            let lines = buffer_grid_lines(&buffer);

            let alert_y = lines
                .iter()
                .position(|line| line.contains("! demo/alert"))
                .expect("alert fleet row should render");
            let alert = buffer_cell_in_line(&buffer, &lines[alert_y], alert_y, "demo/alert");
            assert_eq!(alert.fg, theme.danger, "{preset:?}");
            assert!(alert.modifier.contains(Modifier::BOLD), "{preset:?}");

            let mut waiting_first = sample_pane("bash");
            waiting_first.id = String::from("%1");
            waiting_first.window_name = String::from("idle");

            let mut waiting_second = sample_pane("claude");
            waiting_second.id = String::from("%2");
            waiting_second.window_id = String::from("@2");
            waiting_second.window_name = String::from("waiting");
            waiting_second.active = false;
            waiting_second.pane_index = 1;

            let waiting_app = app_with_panes(
                vec![waiting_first, waiting_second],
                vec![
                    ("%1", vec!["idle"]),
                    ("%2", vec!["Waiting for approval. Continue?"]),
                ],
            );
            let mut waiting_app = waiting_app;
            waiting_app.set_theme_for_test(ThemeConfig {
                preset: Some(preset),
                overrides: ThemeOverrides::default(),
            });
            let theme = Theme::from_settings(waiting_app.ui_settings());
            let buffer = render_buffer(&waiting_app, 120, 18);
            let lines = buffer_grid_lines(&buffer);

            let waiting_y = lines
                .iter()
                .position(|line| line.contains("! demo/waiting"))
                .expect("waiting fleet row should render");
            let waiting =
                buffer_cell_in_line(&buffer, &lines[waiting_y], waiting_y, "demo/waiting");
            assert_eq!(waiting.fg, theme.warning, "{preset:?}");
            assert!(waiting.modifier.contains(Modifier::BOLD), "{preset:?}");

            let mut target_first = sample_pane("bash");
            target_first.id = String::from("%1");
            target_first.window_name = String::from("idle");

            let mut target_second = sample_pane("bash");
            target_second.id = String::from("%2");
            target_second.window_name = String::from("mid");
            target_second.active = false;
            target_second.pane_index = 1;

            let mut target_third = sample_pane("bash");
            target_third.id = String::from("%3");
            target_third.window_name = String::from("target");
            target_third.active = false;
            target_third.pane_index = 2;

            let mut target_app = app_with_panes(
                vec![target_first, target_second, target_third],
                vec![
                    ("%1", vec!["building..."]),
                    ("%2", vec!["building..."]),
                    ("%3", vec!["building..."]),
                ],
            );
            target_app.set_theme_for_test(ThemeConfig {
                preset: Some(preset),
                overrides: ThemeOverrides::default(),
            });
            target_app.select_next_pane();
            target_app.select_next_pane();
            target_app.toggle_selected_mark();
            target_app.select_previous_pane();
            target_app.select_previous_pane();

            let theme = Theme::from_settings(target_app.ui_settings());
            let buffer = render_buffer(&target_app, 120, 18);
            let lines = buffer_grid_lines(&buffer);
            let targeted_y = line_index(&lines, "demo/target");
            let targeted =
                buffer_cell_in_line(&buffer, &lines[targeted_y], targeted_y, "demo/target");
            assert_eq!(targeted.fg, theme.success, "{preset:?}");
            assert!(targeted.modifier.contains(Modifier::BOLD), "{preset:?}");

            let fake_tmux = fake_tmux_script(
                &format!("theme-watching-row-{preset:?}"),
                "#!/bin/sh\nexit 0\n",
            );
            let mut watch_first = sample_pane("codex");
            watch_first.id = String::from("%1");
            watch_first.window_name = String::from("alpha");
            let mut watch_second = sample_pane("claude");
            watch_second.id = String::from("%2");
            watch_second.window_id = String::from("@2");
            watch_second.window_name = String::from("beta");
            watch_second.active = false;
            watch_second.pane_index = 1;
            let mut watch_app = app_with_panes(
                vec![watch_first, watch_second],
                vec![
                    ("%1", vec!["Press Enter to continue."]),
                    ("%2", vec!["Press Enter to continue."]),
                ],
            );
            watch_app.set_theme_for_test(ThemeConfig {
                preset: Some(preset),
                overrides: ThemeOverrides::default(),
            });
            use_fake_tmux_for_test(&mut watch_app, fake_tmux);
            watch_app.show_command_center();
            runtime
                .block_on(handle_key_press(&mut watch_app, KeyCode::Char('a')))
                .expect("Command Center continue should create a watching row");
            assert!(watch_app.go_back());

            let theme = Theme::from_settings(watch_app.ui_settings());
            let buffer = render_buffer(&watch_app, 120, 18);
            let lines = buffer_grid_lines(&buffer);
            let watching_y = line_index(&lines, "demo/alpha");
            let watching =
                buffer_cell_in_line(&buffer, &lines[watching_y], watching_y, "demo/alpha");
            assert_eq!(watching.fg, theme.muted, "{preset:?}");
            assert!(
                watching.modifier.contains(Modifier::BOLD),
                "{preset:?} watching rows need a non-color cue"
            );
        }
    }

    #[test]
    fn default_theme_renderer_keeps_terminal_native_cells() {
        let fixture = panel_fixture("selected_waiting_panel");
        let app = app_from_panel_fixture(&fixture);
        assert_eq!(app.theme_preset(), ThemePreset::TerminalNative);

        let theme = Theme::from_settings(app.ui_settings());
        assert_eq!(theme.text, Color::Reset);
        assert_eq!(theme.selected_bg, Color::Reset);

        let buffer = render_buffer(&app, 120, 18);
        let lines = buffer_grid_lines(&buffer);
        let selected_y = lines
            .iter()
            .position(|line| line.contains(">! demo/agents"))
            .expect("selected attention row should render");
        let selected = buffer_cell_in_line(&buffer, &lines[selected_y], selected_y, "demo/agents");
        assert_eq!(selected.fg, Color::Reset);
        assert_eq!(selected.bg, Color::Reset);
        assert!(selected.modifier.contains(Modifier::BOLD));
        assert!(selected.modifier.contains(Modifier::REVERSED));

        let marker = buffer_cell_in_line(&buffer, &lines[selected_y], selected_y, "!");
        assert_eq!(marker.fg, theme.warning);
        assert_eq!(marker.bg, Color::Reset);
        assert!(marker.modifier.contains(Modifier::BOLD));
        assert!(marker.modifier.contains(Modifier::REVERSED));

        let footer = buffer_cell(&buffer, &lines, "? help");
        assert_eq!(footer.fg, Color::Reset);
        assert_eq!(footer.bg, Color::Reset);
    }

    #[test]
    fn usability_theme_scrollbars_use_accent_and_surface_slots() {
        for preset in ALL_THEME_PRESETS {
            let output = (1..=40)
                .map(|index| format!("theme output {index:02}"))
                .collect::<Vec<_>>();
            let output_refs = output.iter().map(String::as_str).collect::<Vec<_>>();
            let mut app = app_with_panes(vec![sample_pane("bash")], vec![("%1", output_refs)]);
            app.set_theme_for_test(ThemeConfig {
                preset: Some(preset),
                overrides: ThemeOverrides::default(),
            });
            app.cycle_panel_focus();

            let theme = Theme::from_settings(app.ui_settings());
            let buffer = render_buffer(&app, 120, 20);
            let lines = buffer_grid_lines(&buffer);
            let cells = scrollbar_cells(&lines);
            assert!(!cells.is_empty(), "{preset:?}\n{}", screen_text(&lines));

            let thumb = cells
                .iter()
                .find(|(_, _, ch)| *ch == '█')
                .expect("scrollbar thumb should render");
            let thumb_cell = buffer
                .cell((thumb.1 as u16, thumb.0 as u16))
                .expect("thumb cell should exist");
            assert_eq!(thumb_cell.fg, theme.accent, "{preset:?}");

            let track = cells
                .iter()
                .find(|(_, _, ch)| *ch == '░')
                .expect("scrollbar track should render");
            let track_cell = buffer
                .cell((track.1 as u16, track.0 as u16))
                .expect("track cell should exist");
            assert_eq!(track_cell.fg, theme.surface, "{preset:?}");
        }
    }

    #[test]
    fn usability_theme_onboarding_picker_is_readable_minimal_and_keyboard_obvious() {
        let mut app = app_with_panes(vec![sample_pane("codex")], vec![]);
        app.open_theme_picker(true);

        let buffer = render_buffer(&app, 96, 18);
        let lines = buffer_grid_lines(&buffer);
        let screen = screen_text(&lines);
        let footer = lines.last().expect("footer should render");
        let theme = Theme::from_settings(app.ui_settings());

        assert_render_invariants(&lines, 96, 18);
        assert!(screen.contains("muxboard"), "{screen}");
        assert!(screen.contains("Choose a theme."), "{screen}");
        assert!(screen.contains("Pick a look."), "{screen}");
        for choice in [
            "> System Colors",
            "Light",
            "Dark",
            "More themes",
            "match your terminal palette",
            "clean palette",
        ] {
            assert!(screen.contains(choice), "missing `{choice}`:\n{screen}");
        }
        for noisy in [
            "ui_settings",
            "theme_preset",
            "JSON",
            "tutorial",
            "settings",
        ] {
            assert!(
                !screen.contains(noisy),
                "theme onboarding should not leak setup jargon `{noisy}`:\n{screen}"
            );
        }
        assert!(footer.contains("J/K choose"), "{screen}");
        assert!(footer.contains("Enter save"), "{screen}");
        assert!(footer.contains("Esc system"), "{screen}");
        assert!(footer.to_ascii_lowercase().contains("q quit"), "{screen}");
        assert!(
            !footer.contains("? help"),
            "the picker footer should not advertise another surface:\n{screen}"
        );

        let (_, intro_y) = text_position(&lines, "Pick a look.");
        let (_, system_y) = text_position(&lines, "> System Colors");
        let (_, light_y) = text_position(&lines, "Light");
        let (_, dark_y) = text_position(&lines, "Dark");
        assert!(
            intro_y < system_y && system_y < light_y && light_y < dark_y,
            "{screen}"
        );

        let selected = buffer_cell(&buffer, &lines, "System Colors");
        assert_eq!(selected.fg, theme.accent);
        assert!(selected.modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn usability_theme_picker_more_page_stays_scannable() {
        let runtime = tokio::runtime::Runtime::new().expect("runtime should build");
        let mut app = app_with_panes(vec![sample_pane("codex")], vec![]);
        app.open_theme_picker(true);

        for _ in 0..3 {
            runtime
                .block_on(handle_key_press(&mut app, KeyCode::Char('j')))
                .expect("theme picker should move");
        }
        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Enter))
            .expect("theme picker should open more themes");

        let lines = render_grid(&app, 96, 18);
        let screen = screen_text(&lines);
        let footer = lines.last().expect("footer should render");

        assert_render_invariants(&lines, 96, 18);
        assert!(screen.contains("More themes"), "{screen}");
        for choice in [
            "> Catppuccin Latte",
            "Tokyo Night",
            "Gruvbox Dark",
            "Nord",
            "Rose Pine",
            "Back",
        ] {
            assert!(screen.contains(choice), "missing `{choice}`:\n{screen}");
        }
        assert!(footer.contains("J/K choose"), "{screen}");
        assert!(footer.contains("Enter save"), "{screen}");
        assert!(footer.contains("Esc system"), "{screen}");
        assert!(!screen.contains("theme_preset"), "{screen}");
    }

    #[test]
    fn usability_action_contract_theme_onboarding_keys_match_the_footer() {
        let runtime = tokio::runtime::Runtime::new().expect("runtime should build");

        let mut dark = app_with_panes(vec![sample_pane("codex")], vec![]);
        dark.open_theme_picker(true);
        runtime
            .block_on(handle_key_press(&mut dark, KeyCode::Char('j')))
            .expect("move to light should work");
        runtime
            .block_on(handle_key_press(&mut dark, KeyCode::Char('j')))
            .expect("move to dark should work");
        runtime
            .block_on(handle_key_press(&mut dark, KeyCode::Enter))
            .expect("enter should save dark");
        assert!(!dark.is_theme_picker_active());
        assert_eq!(dark.theme_preset(), ThemePreset::CatppuccinMocha);
        assert_eq!(dark.status_message(), "Theme: Dark.");
        let dark_lines = render_grid(&dark, 80, 18);
        let dark_footer = dark_lines.last().expect("footer should render");
        assert!(
            dark_footer.contains("Theme: Dark."),
            "{}",
            screen_text(&dark_lines)
        );
        assert!(
            dark_footer.contains("J/K move"),
            "{}",
            screen_text(&dark_lines)
        );
        assert!(
            dark_footer.contains("Enter output"),
            "{}",
            screen_text(&dark_lines)
        );
        assert!(
            dark_footer.contains(": send"),
            "{}",
            screen_text(&dark_lines)
        );

        let mut terminal = app_with_panes(vec![sample_pane("codex")], vec![]);
        terminal.open_theme_picker(true);
        runtime
            .block_on(handle_key_press(&mut terminal, KeyCode::Enter))
            .expect("enter should save system colors");
        assert!(!terminal.is_theme_picker_active());
        assert_eq!(terminal.theme_preset(), ThemePreset::TerminalNative);
        assert_eq!(terminal.status_message(), "Theme: System Colors.");

        let mut escape = app_with_panes(vec![sample_pane("codex")], vec![]);
        escape.open_theme_picker(true);
        runtime
            .block_on(handle_key_press(&mut escape, KeyCode::Esc))
            .expect("escape should keep system colors");
        assert!(!escape.is_theme_picker_active());
        assert_eq!(escape.theme_preset(), ThemePreset::TerminalNative);
        assert_eq!(escape.status_message(), "Theme: System Colors.");

        let mut existing = app_with_panes(vec![sample_pane("codex")], vec![]);
        existing.set_theme_for_test(ThemeConfig {
            preset: Some(ThemePreset::TokyoNight),
            overrides: ThemeOverrides::default(),
        });
        existing.open_theme_picker(false);
        assert!(
            existing.status_hint_line_for_width(80).contains("Esc keep"),
            "{}",
            existing.status_hint_line_for_width(80)
        );
        runtime
            .block_on(handle_key_press(&mut existing, KeyCode::Esc))
            .expect("escape should keep existing theme");
        assert!(!existing.is_theme_picker_active());
        assert_eq!(existing.theme_preset(), ThemePreset::TokyoNight);
        assert_eq!(existing.status_message(), "Kept current theme.");

        let mut more = app_with_panes(vec![sample_pane("codex")], vec![]);
        more.open_theme_picker(true);
        for _ in 0..3 {
            runtime
                .block_on(handle_key_press(&mut more, KeyCode::Char('j')))
                .expect("move to more should work");
        }
        runtime
            .block_on(handle_key_press(&mut more, KeyCode::Enter))
            .expect("enter should open more themes");
        runtime
            .block_on(handle_key_press(&mut more, KeyCode::Esc))
            .expect("escape should return to simple choices");
        assert!(more.is_theme_picker_active());
        assert!(more.theme_picker_lines()[1].contains("> System Colors"));

        let mut quit = app_with_panes(vec![sample_pane("codex")], vec![]);
        quit.open_theme_picker(true);
        runtime
            .block_on(handle_key_press(&mut quit, KeyCode::Char('q')))
            .expect("q should quit safely");
        assert!(quit.should_quit());
    }

    #[test]
    fn wide_screen_renders_board_then_selected_inspector_with_clear_hierarchy() {
        let fixture = panel_fixture("selected_waiting_panel");
        let app = app_from_panel_fixture(&fixture);
        let lines = render_lines(&app, 120, 18);
        let screen = screen_text(&lines);

        assert_no_garbled_text(&lines);
        assert!(lines[0].contains("muxboard"), "{screen}");
        assert!(screen.contains("demo/agents"), "{screen}");
        assert!(screen.contains("1-1 / 1  1 needs you"), "{screen}");
        assert!(screen.contains("Fleet | 1-1 / 1 | 1 needs you"), "{screen}");
        assert!(screen.contains("Details"), "{screen}");
        assert!(
            screen.contains("State: Waiting   Tool: Claude Code"),
            "{screen}"
        );
        assert!(screen.contains("Blocked: network access"), "{screen}");
        assert!(screen.contains("Action: : reply"), "{screen}");
        assert!(!screen.contains("STATUS="), "{screen}");
        assert!(
            lines.last().is_some_and(|line| line.contains("? help")),
            "{screen}"
        );

        let fleet_line = line_index(&lines, "Fleet | 1-1 / 1 | 1 needs you");
        let selected_line = line_index(&lines, "Details");
        assert!(selected_line >= fleet_line);
    }

    #[test]
    fn selected_inspector_renders_distilled_progress_without_raw_duplicate_tail() {
        let pane = sample_pane("bash");
        let app = app_with_panes(vec![pane], vec![("%1", vec!["building release artifacts"])]);
        let lines = render_grid(&app, 120, 18);
        let screen = screen_text(&lines);

        assert_render_invariants(&lines, 120, 18);
        assert!(screen.contains("Details"), "{screen}");
        assert!(!screen.contains("Output"), "{screen}");
        assert!(screen.contains("build release artifacts"), "{screen}");
        assert!(!screen.contains("building release artifacts"), "{screen}");
    }

    #[test]
    fn selected_inspector_error_keeps_blocker_and_action_above_output() {
        let pane = sample_pane("bash");
        let app = app_with_panes(vec![pane], vec![("%1", vec!["error: command failed"])]);
        let lines = render_grid(&app, 120, 18);
        let screen = screen_text(&lines);

        assert_render_invariants(&lines, 120, 18);
        assert!(screen.contains("State: Error"), "{screen}");
        assert!(screen.contains("Problem: command failed"), "{screen}");
        assert!(screen.contains("Action: Enter output"), "{screen}");
        assert!(!screen.contains("Agent report"), "{screen}");
        assert!(
            line_index(&lines, "Problem:") < line_index(&lines, "Action:"),
            "{screen}"
        );
    }

    #[test]
    fn selected_inspector_done_keeps_result_above_metadata() {
        let pane = sample_pane("bash");
        let app = app_with_panes(vec![pane], vec![("%1", vec!["completed successfully"])]);
        let lines = render_grid(&app, 120, 18);
        let screen = screen_text(&lines);

        assert_render_invariants(&lines, 120, 18);
        assert!(screen.contains("State: Done"), "{screen}");
        assert!(screen.contains("Now:"), "{screen}");
        assert!(screen.contains("Updated:"), "{screen}");
        assert!(
            line_index(&lines, "Now:") < line_index(&lines, "Updated:"),
            "{screen}"
        );
    }

    #[test]
    fn selected_inspector_keeps_a_useful_output_slice_before_activity_metadata() {
        let pane = sample_pane("bash");
        let app = app_with_panes(
            vec![pane],
            vec![(
                "%1",
                vec![
                    "download dependencies",
                    "compile crate",
                    "run unit tests",
                    "package binary",
                    "upload artifact",
                    "notify operator",
                    "done release",
                ],
            )],
        );

        for &(width, height) in &[(120, 18), (100, 16)] {
            let lines = render_grid(&app, width, height);
            let screen = screen_text(&lines);
            assert_render_invariants(&lines, width, height);
            assert!(screen.contains("Details"), "{width}x{height}\n{screen}");
            assert!(screen.contains("Output"), "{width}x{height}\n{screen}");
            assert!(
                screen.contains("package binary"),
                "{width}x{height}\n{screen}"
            );
            assert!(
                screen.contains("upload artifact"),
                "{width}x{height}\n{screen}"
            );
            assert!(
                screen.contains("notify operator"),
                "{width}x{height}\n{screen}"
            );
            assert!(
                screen.contains("done release"),
                "{width}x{height}\n{screen}"
            );
            if screen.contains("Updated:") {
                assert!(
                    line_index(&lines, "Output") < line_index(&lines, "Updated:"),
                    "{width}x{height}\n{screen}"
                );
            }
        }
    }

    #[test]
    fn cramped_selected_inspector_keeps_output_lines_before_activity_metadata() {
        let pane = sample_pane("bash");
        let app = app_with_panes(
            vec![pane],
            vec![(
                "%1",
                vec![
                    "download dependencies",
                    "compile crate",
                    "run unit tests",
                    "package binary",
                    "upload artifact",
                    "notify operator",
                    "done release",
                ],
            )],
        );

        let raw_lines = app.inspector_lines();
        let expected = prepare_context_panel_lines("Details", raw_lines, 55, 10);
        let lines = render_grid(&app, 109, 14);
        let screen = screen_text(&lines);

        assert_render_invariants(&lines, 109, 14);
        assert!(screen.contains("Output"), "{screen}");
        assert!(screen.contains("notify operator"), "{screen}");
        assert!(screen.contains("done release"), "{screen}");
        assert!(
            !screen.contains("Updated:"),
            "scarce Details rows should go to output before activity metadata:\n{screen}"
        );
        assert!(
            expected
                .iter()
                .filter(|line| line.starts_with("  "))
                .count()
                >= 2
        );
    }

    #[test]
    fn selected_inspector_keeps_more_output_before_metadata_when_space_allows() {
        let pane = sample_pane("bash");
        let app = app_with_panes(
            vec![pane],
            vec![(
                "%1",
                vec![
                    "step 01 plan",
                    "step 02 fetch",
                    "step 03 build",
                    "step 04 test",
                    "step 05 lint",
                    "step 06 package",
                    "step 07 upload",
                    "step 08 notify",
                    "step 09 verify",
                    "step 10 done",
                ],
            )],
        );

        let lines = render_grid(&app, 120, 22);
        let screen = screen_text(&lines);

        assert_render_invariants(&lines, 120, 22);
        assert!(screen.contains("Output"), "{screen}");
        assert!(screen.contains("lint"), "{screen}");
        assert!(screen.contains("package"), "{screen}");
        assert!(screen.contains("upload"), "{screen}");
        assert!(screen.contains("notify"), "{screen}");
        assert!(screen.contains("verify"), "{screen}");
        assert!(screen.contains("done"), "{screen}");
        assert!(
            line_index(&lines, "Output") < line_index(&lines, "Updated:"),
            "{screen}"
        );
    }

    #[test]
    fn selected_inspector_hides_empty_updated_metadata() {
        let pane = sample_pane("bash");
        let app = app_with_panes(vec![pane], vec![]);

        let lines = render_grid(&app, 120, 18);
        let screen = screen_text(&lines);

        assert_render_invariants(&lines, 120, 18);
        assert!(screen.contains("Details"), "{screen}");
        assert!(!screen.contains("Updated: no output yet"), "{screen}");
        assert!(!screen.contains("Updated:"), "{screen}");
    }

    #[test]
    fn selected_inspector_does_not_render_attention_state_as_updated_metadata() {
        let pane = sample_pane("claude");
        let mut app = app_with_panes(vec![pane], vec![]);
        set_runtime_lines_without_age(
            &mut app,
            "%1",
            &[
                "Dialog open: Allow command? [y/n]",
                "Worker request: run cargo test usability_",
            ],
            "claude dialog open allow command worker request run cargo test usability",
        );

        let lines = render_grid(&app, 120, 18);
        let screen = screen_text(&lines);

        assert_render_invariants(&lines, 120, 18);
        assert!(screen.contains("State: Waiting"), "{screen}");
        assert!(screen.contains("Action: . answer yes/no"), "{screen}");
        assert!(!screen.contains("Updated: awaiting input"), "{screen}");
        assert!(!screen.contains("Updated:"), "{screen}");
    }

    #[test]
    fn narrow_selected_inspector_keeps_next_before_any_output() {
        let fixture = panel_fixture("selected_waiting_panel");
        let app = app_from_panel_fixture(&fixture);
        let lines = render_grid(&app, 70, 16);
        let screen = screen_text(&lines);

        assert_render_invariants(&lines, 70, 16);
        assert!(screen.contains("Details"), "{screen}");
        assert!(
            line_index(&lines, "Blocked:") < line_index(&lines, "Action:"),
            "{screen}"
        );
        if screen.contains("Output") {
            assert!(
                line_index(&lines, "Action:") < line_index(&lines, "Output"),
                "{screen}"
            );
        }
    }

    #[test]
    fn narrow_screen_keeps_board_scannable_and_stacks_selected_details_under_it() {
        let fixture = panel_fixture("selected_waiting_panel");
        let app = app_from_panel_fixture(&fixture);
        let lines = render_lines(&app, 70, 16);
        let screen = screen_text(&lines);

        assert_no_garbled_text(&lines);
        assert!(screen.contains("Fleet | 1-1 / 1 | 1 needs you"), "{screen}");
        assert!(screen.contains("Where"), "{screen}");
        assert!(screen.contains("State"), "{screen}");
        assert!(screen.contains("demo/agents"), "{screen}");
        assert!(screen.contains("network access"), "{screen}");
        assert!(!screen.contains("claude: network access"), "{screen}");

        let board_line = line_index(&lines, "Fleet | 1-1 / 1 | 1 needs you");
        let selected_line = line_index(&lines, "Details");
        assert!(selected_line > board_line);
    }

    #[test]
    fn actions_overlay_centers_secondary_actions_without_hiding_board_context() {
        let fixture = panel_fixture("actions_menu_sections");
        let app = app_from_panel_fixture(&fixture);
        let lines = render_lines(&app, 110, 20);
        let screen = screen_text(&lines);

        assert_no_garbled_text(&lines);
        assert!(screen.contains("More"), "{screen}");
        assert!(screen.contains("View"), "{screen}");
        assert!(screen.contains("Space add to send list"), "{screen}");
        assert!(!screen.contains("Details"), "{screen}");
        assert!(line_index(&lines, "More") < 5, "{screen}");
    }

    #[test]
    fn usability_more_lists_the_primary_action_before_secondary_peeks() {
        let fixture = panel_fixture("actions_menu_sections");
        let app = app_from_panel_fixture(&fixture);
        let lines = render_grid(&app, 110, 20);
        let screen = screen_text(&lines);

        assert_render_invariants(&lines, 110, 20);
        assert!(screen.contains("Action: : send this pane"), "{screen}");
        assert!(
            line_index(&lines, ": send text") < line_index(&lines, "Enter show output"),
            "More should list the primary send action before secondary Output:\n{screen}"
        );

        let mut reply = app_with_panes(
            vec![sample_pane("codex")],
            vec![("%1", vec!["Type your answer to continue."])],
        );
        reply.open_action_menu();

        let reply_lines = render_grid(&reply, 110, 20);
        let reply_screen = screen_text(&reply_lines);

        assert_render_invariants(&reply_lines, 110, 20);
        assert!(reply_screen.contains("Action: : reply"), "{reply_screen}");
        assert!(
            line_index(&reply_lines, ": reply") < line_index(&reply_lines, "Enter show output"),
            "More should list reply before a secondary Output peek:\n{reply_screen}"
        );
    }

    #[test]
    fn output_view_shows_distilled_summary_before_raw_tail() {
        let fixture = panel_fixture("live_tail_with_summary_and_raw_tail");
        let app = app_from_panel_fixture(&fixture);
        let lines = render_lines(&app, 110, 18);
        let screen = screen_text(&lines);

        assert_no_garbled_text(&lines);
        assert!(screen.contains("Output"), "{screen}");
        assert!(screen.contains("Summary"), "{screen}");
        assert!(screen.contains("write tests"), "{screen}");
        assert!(!screen.contains("Latest"), "{screen}");
        assert!(!screen.contains("STATUS=running"), "{screen}");
        assert!(!screen.contains("│  codex"), "{screen}");
        assert!(!screen.contains("Details"), "{screen}");
        assert!(line_index(&lines, "Output") < 5, "{screen}");
    }

    #[test]
    fn output_overlay_keeps_waiting_prompt_context_without_duplicate_summary() {
        let pane = sample_pane("claude");
        let mut app = app_with_panes(
            vec![pane],
            vec![(
                "%1",
                vec!["Waiting for leader to approve network access to api.example.com"],
            )],
        );
        app.cycle_context_pane();
        let lines = render_grid(&app, 110, 18);
        let screen = screen_text(&lines);

        assert_render_invariants(&lines, 110, 18);
        assert!(screen.contains("Summary"), "{screen}");
        assert!(screen.contains("network access"), "{screen}");
        assert!(screen.contains("Latest"), "{screen}");
        assert!(
            screen.contains("Waiting for leader to approve network access"),
            "{screen}"
        );
        assert!(
            line_index(&lines, "Summary") < line_index(&lines, "Latest"),
            "{screen}"
        );
    }

    #[test]
    fn output_overlay_preserves_answer_prompts_exactly() {
        let pane = sample_pane("opencode");
        let mut app = app_with_panes(vec![pane], vec![("%1", vec!["Type your answer..."])]);
        app.cycle_context_pane();
        let lines = render_grid(&app, 100, 16);
        let screen = screen_text(&lines);

        assert_render_invariants(&lines, 100, 16);
        assert!(screen.contains("Type your answer..."), "{screen}");
    }

    #[test]
    fn wide_board_shows_dedicated_tool_column_for_generic_launchers() {
        let pane = sample_pane("node");
        let app = app_with_panes(
            vec![pane],
            vec![(
                "%1",
                vec!["codex", "STATUS=running | BLOCKER=none | NEXT=write tests"],
            )],
        );
        let lines = render_lines(&app, 220, 18);
        let screen = screen_text(&lines);

        assert_no_garbled_text(&lines);
        assert!(screen.contains("Tool"), "{screen}");
        assert!(screen.contains("codex"), "{screen}");
        assert!(screen.contains("write tests"), "{screen}");
        assert!(!screen.contains("STATUS=running"), "{screen}");
    }

    #[test]
    fn standard_board_prefixes_latest_with_tool_when_no_tool_column_is_visible() {
        let pane = sample_pane("node");
        let app = app_with_panes(
            vec![pane],
            vec![(
                "%1",
                vec!["codex", "STATUS=running | BLOCKER=none | NEXT=write tests"],
            )],
        );
        let lines = render_lines(&app, 84, 16);
        let screen = screen_text(&lines);

        assert_no_garbled_text(&lines);
        assert!(!screen.contains("│   Where          Tool"), "{screen}");
        assert!(screen.contains("codex: write tests"), "{screen}");
    }

    #[test]
    fn split_board_keeps_plain_session_window_locations_readable() {
        let mut codex = sample_pane("codex");
        codex.id = String::from("%1");
        codex.session_name = String::from("muxdog");
        codex.window_id = String::from("@1");
        codex.window_name = String::from("codex");

        let mut claude = sample_pane("claude");
        claude.id = String::from("%2");
        claude.session_name = String::from("muxdog");
        claude.window_id = String::from("@2");
        claude.window_name = String::from("claude");
        claude.pane_index = 1;
        claude.active = false;

        let mut opencode = sample_pane("opencode");
        opencode.id = String::from("%3");
        opencode.session_name = String::from("muxdog");
        opencode.window_id = String::from("@3");
        opencode.window_name = String::from("opencode");
        opencode.pane_index = 2;
        opencode.active = false;

        let app = app_with_panes(
            vec![codex, claude, opencode],
            vec![
                ("%1", vec!["Waiting for approval. Continue?"]),
                ("%2", vec!["error: command failed"]),
                ("%3", vec!["Building renderer tests..."]),
            ],
        );
        let lines = render_lines(&app, 120, 36);
        let screen = screen_text(&lines);

        assert_no_garbled_text(&lines);
        assert!(
            screen.contains("muxdog/claude"),
            "roomy split view should not abbreviate a simple selected location:\n{screen}"
        );
        assert!(
            !screen.contains("muxdog/cl..."),
            "escaped dogfood truncation regressed:\n{screen}"
        );
        assert!(screen.contains("failed: command failed"), "{screen}");
        assert!(screen.contains("needs you: approval needed"), "{screen}");
    }

    #[test]
    fn standard_board_puts_error_detail_before_provider_identity_on_narrow_rows() {
        let pane = sample_pane("node");
        let app = app_with_panes(
            vec![pane],
            vec![("%1", vec!["codex", "error: command failed"])],
        );
        let lines = render_lines(&app, 68, 16);
        let screen = screen_text(&lines);

        assert_no_garbled_text(&lines);
        assert!(screen.contains("demo/agents"), "{screen}");
        assert!(screen.contains("failed"), "{screen}");
        assert!(screen.contains("command failed"), "{screen}");
        assert!(!screen.contains("codex: command failed"), "{screen}");
    }

    #[test]
    fn standard_board_surfaces_direct_continue_for_press_enter_prompts() {
        let pane = sample_pane("bash");
        let app = app_with_panes(vec![pane], vec![("%1", vec!["Press Enter to continue."])]);
        let lines = render_lines(&app, 68, 16);
        let screen = screen_text(&lines);
        let board_row = &lines[3];

        assert_no_garbled_text(&lines);
        assert!(screen.contains("demo/agents"), "{screen}");
        assert!(board_row.contains("needs you continue"), "{screen}");
        assert!(!board_row.contains("approval needed"), "{screen}");
        assert!(!board_row.contains("review request"), "{screen}");
    }

    #[test]
    fn standard_board_prefers_specific_progress_over_generic_running_marker() {
        let pane = sample_pane("codex");
        let app = app_with_panes(
            vec![pane],
            vec![("%1", vec!["Running", "building release artifacts"])],
        );
        let lines = render_lines(&app, 68, 16);
        let screen = screen_text(&lines);
        let board_row = &lines[3];

        assert_no_garbled_text(&lines);
        assert!(screen.contains("demo/agents"), "{screen}");
        assert_terms_in_order(board_row, &["working", "codex: build release artifacts"]);
        assert!(!board_row.contains("continue work"), "{screen}");
    }

    #[test]
    fn renderer_surfaces_unseen_done_agent_as_review_attention() {
        let mut pane = sample_pane("node");
        mark_pane_done_for_review(
            &mut pane,
            "codex",
            "release ready",
            "Ship V1",
            "10/10 tests",
        );
        let app = app_with_panes(vec![pane], vec![("%1", vec!["plain node harness output"])]);
        let lines = render_grid(&app, 100, 20);
        let screen = screen_text(&lines);

        assert_render_invariants(&lines, 100, 20);
        assert_no_garbled_text(&lines);
        assert!(screen.contains("1 needs you"), "{screen}");
        assert!(screen.contains("review"), "{screen}");
        assert!(screen.contains("codex: release ready"), "{screen}");
        assert!(!screen.contains("all quiet"), "{screen}");
    }

    #[test]
    fn renderer_surfaces_native_agent_progress_without_provider_duplication() {
        let mut pane = sample_pane("node");
        pane.title = String::from("Claude Code");
        mark_pane_running_agent(
            &mut pane,
            "claude",
            "working",
            "Improve command center",
            "tightening Fleet and Details hierarchy",
        );
        let app = app_with_panes(vec![pane], vec![("%1", vec!["plain node harness output"])]);
        let lines = render_grid(&app, 124, 22);
        let screen = screen_text(&lines);

        assert_render_invariants(&lines, 124, 22);
        assert_no_garbled_text(&lines);
        assert!(screen.contains("claude working"), "{screen}");
        assert!(
            screen.contains("tightening Fleet and Details hierarchy"),
            "{screen}"
        );
        assert!(screen.contains("Improve command center"), "{screen}");
        assert!(
            screen.contains("Now: working · tightening Fleet"),
            "{screen}"
        );
        assert!(!screen.contains("Claude Code · Claude Code"), "{screen}");
    }

    #[test]
    fn standard_board_trades_pane_width_for_latest_when_locations_are_long() {
        let mut pane = sample_pane("claude");
        pane.session_name = String::from("very-long-session-name");
        pane.window_name = String::from("ridiculously-long-window-name");

        let app = app_with_panes(
            vec![pane],
            vec![(
                "%1",
                vec!["STATUS=waiting | BLOCKER=network access | NEXT=approve request"],
            )],
        );
        let lines = render_lines(&app, 68, 16);
        let screen = screen_text(&lines);
        let board_row = &lines[3];

        assert_no_garbled_text(&lines);
        assert!(screen.contains("network access"), "{screen}");
        assert!(!screen.contains("claude: network access"), "{screen}");
        assert!(board_row.contains("very-long-ses..."), "{screen}");
        assert!(
            !board_row.contains("ridiculously-long-window-name"),
            "{screen}"
        );
    }

    #[test]
    fn duplicate_window_rows_show_pane_indexes_to_disambiguate() {
        let mut first = sample_pane("node");
        first.id = String::from("%1");
        first.pane_index = 0;

        let mut second = sample_pane("node");
        second.id = String::from("%2");
        second.pane_index = 1;
        second.active = false;

        let app = app_with_panes(
            vec![first, second],
            vec![
                (
                    "%1",
                    vec!["codex", "STATUS=running | BLOCKER=none | NEXT=write tests"],
                ),
                (
                    "%2",
                    vec![
                        "claude",
                        "STATUS=waiting | BLOCKER=network access | NEXT=approve request",
                    ],
                ),
            ],
        );
        let lines = render_lines(&app, 96, 16);
        let screen = screen_text(&lines);

        assert_no_garbled_text(&lines);
        assert!(screen.contains("#0"), "{screen}");
        assert!(screen.contains("#1"), "{screen}");
    }

    #[test]
    fn confirm_send_render_surfaces_confirmation_in_header_footer_and_overlay() {
        let fixture = view_fixture("confirm_dispatch_header_and_footer");
        let app = app_from_view_model_fixture(&fixture);
        let lines = render_lines(&app, 100, 18);
        let screen = screen_text(&lines);

        assert_no_garbled_text(&lines);
        assert!(
            lines[0].contains("Review send to fleet triage (2 panes)."),
            "{screen}"
        );
        assert!(screen.contains("Send"), "{screen}");
        assert!(screen.contains("To: fleet triage (2 panes)"), "{screen}");
        assert!(screen.contains("Text: continue"), "{screen}");
        assert!(
            lines
                .last()
                .is_some_and(|line| line.contains("Enter send  Esc cancel")),
            "{screen}"
        );
    }

    #[test]
    fn usability_confirm_send_avoids_duplicate_target_counts() {
        let app = app_from_view_model_fixture(&view_fixture("confirm_dispatch_header_and_footer"));
        let lines = render_grid(&app, 100, 18);
        let screen = screen_text(&lines);

        assert_render_invariants(&lines, 100, 18);
        assert!(
            screen.contains("Review send to fleet triage (2 panes)."),
            "{screen}"
        );
        assert!(screen.contains("To: fleet triage (2 panes)"), "{screen}");
        assert!(!screen.contains("send to 2 panes in fleet"), "{screen}");
        assert!(!screen.contains("pane(s)"), "{screen}");
        assert!(!screen.contains("group(s)"), "{screen}");
    }

    #[test]
    fn header_keeps_brand_and_summary_on_one_dense_row() {
        let fixture = panel_fixture("selected_waiting_panel");
        let app = app_from_panel_fixture(&fixture);

        for &((width, height), summary) in &[
            ((120, 18), "demo/agents  1-1 / 1  1 needs you"),
            ((70, 16), "agents  1-1 / 1  1 needs you"),
        ] {
            let lines = render_grid(&app, width, height);
            let header = &lines[0];
            let summary_x = header.find(summary).unwrap_or_else(|| {
                panic!(
                    "missing summary in {}x{}:
{}",
                    width,
                    height,
                    screen_text(&lines)
                )
            });

            assert_render_invariants(&lines, width, height);
            assert_eq!(
                header.find("muxboard"),
                Some(0),
                "{}x{}:
{}",
                width,
                height,
                screen_text(&lines)
            );
            assert_eq!(
                summary_x + summary.chars().count(),
                width as usize,
                "{}x{}:
{}",
                width,
                height,
                screen_text(&lines)
            );
            assert!(
                lines[1].starts_with('┌'),
                "{}x{}:
{}",
                width,
                height,
                screen_text(&lines)
            );
        }
    }

    #[test]
    fn split_layout_keeps_fleet_and_selected_widths_balanced() {
        let fixture = panel_fixture("selected_waiting_panel");
        let app = app_from_panel_fixture(&fixture);

        for &(width, height) in &[(112, 18), (120, 18), (140, 24)] {
            let lines = render_grid(&app, width, height);
            let spans = panel_spans(&lines[1]);
            let screen = screen_text(&lines);

            assert_render_invariants(&lines, width, height);
            assert_eq!(
                spans.len(),
                2,
                "{}x{}:
{}",
                width,
                height,
                screen
            );

            let (board_start, board_end) = spans[0];
            let (selected_start, selected_end) = spans[1];
            let board_width = board_end - board_start + 1;
            let selected_width = selected_end - selected_start + 1;

            assert_eq!(
                selected_start,
                board_end + 2,
                "{}x{}:
{}",
                width,
                height,
                screen
            );
            assert!(
                board_width * 100 >= selected_width * 80,
                "{}x{} board={} selected={}
{}",
                width,
                height,
                board_width,
                selected_width,
                screen
            );
            assert!(
                selected_width * 100 >= board_width * 80,
                "{}x{} board={} selected={}
{}",
                width,
                height,
                board_width,
                selected_width,
                screen
            );
            assert!(
                lines[3].contains("State: Waiting   Tool: Claude Code"),
                "{}x{}:
{}",
                width,
                height,
                screen
            );
        }
    }

    #[test]
    fn split_board_truncates_pane_before_latest_context_and_keeps_selected_detail() {
        let mut pane = sample_pane("node");
        pane.session_name = String::from("prod");
        pane.window_name = String::from("agent-review-super-long");
        pane.current_path = String::from("/workspace/agent-review-super-long");

        let app = app_with_panes(
            vec![pane],
            vec![(
                "%1",
                vec![
                    "codex",
                    "STATUS=running | BLOCKER=none | NEXT=ship release after validation",
                ],
            )],
        );
        let lines = render_grid(&app, 120, 18);
        let screen = screen_text(&lines);
        let row = lines
            .iter()
            .find(|line| line.contains("prod/agent-re..."))
            .unwrap_or_else(|| {
                panic!(
                    "missing truncated board row:
{screen}"
                )
            });
        let spans = panel_spans(&lines[1]);
        let board_end = spans[0].1;
        let latest_x = row.find("ship release after").unwrap_or_else(|| {
            panic!(
                "missing latest prefix in row:
{row}"
            )
        });

        assert_render_invariants(&lines, 120, 18);
        assert!(!row.contains("prod/agent-review-super-long"), "{row}");
        assert!(row.contains("ship release after"), "{row}");
        assert!(latest_x < board_end, "board_end={board_end} row={row}");
        assert!(screen.contains("prod/agent-review-super-long"), "{screen}");
        assert!(screen.contains("State: Running   Tool: Codex"), "{screen}");
    }

    #[test]
    fn compact_and_standard_thresholds_rebudget_board_columns_cleanly() {
        let fixture = panel_fixture("selected_waiting_panel");
        let app = app_from_panel_fixture(&fixture);

        let compact = render_grid(&app, 67, 16);
        let standard = render_grid(&app, 68, 16);
        let compact_screen = screen_text(&compact);
        let standard_screen = screen_text(&standard);

        assert_render_invariants(&compact, 67, 16);
        assert_render_invariants(&standard, 68, 16);

        assert!(compact[2].contains("Where"));
        assert!(compact[2].contains("Latest"));
        assert!(!compact[2].contains("Now"));
        assert!(compact[3].contains("needs you: network access"));
        assert!(!compact[3].contains("wait:"), "{compact_screen}");
        assert!(!compact[3].contains("claude: network access"));

        assert!(standard[2].contains("Where"));
        assert!(standard[2].contains("Now"));
        assert!(standard[2].contains("Latest"));
        assert!(!standard[2].contains("Tool"));
        assert_terms_in_order(&standard[2], &["Where", "Now", "Latest"]);
        assert!(standard[3].contains("needs you network access"));
        assert!(!standard[3].contains("claude: network access"));

        assert!(compact_screen.contains("┌Fleet"));
        assert!(standard_screen.contains("┌Fleet"));
    }

    #[test]
    fn full_mode_split_layout_renders_tool_state_and_latest_as_distinct_columns() {
        let pane = sample_pane("node");
        let app = app_with_panes(
            vec![pane],
            vec![(
                "%1",
                vec!["codex", "STATUS=running | BLOCKER=none | NEXT=write tests"],
            )],
        );
        let lines = render_grid(&app, 220, 18);
        let screen = screen_text(&lines);
        let board_header = &lines[2];
        let board_row = &lines[3];

        assert_render_invariants(&lines, 220, 18);
        assert_terms_in_order(board_header, &["Where", "Tool", "Now", "Latest"]);
        assert_terms_in_order(
            board_row,
            &["demo/agents", "codex", "working", "write tests"],
        );
        assert!(board_row.contains("demo/agents"));
        assert!(board_row.contains("codex"));
        assert!(board_row.contains("working"));
        assert!(board_row.contains("write tests"));
        assert!(!board_row.contains("codex: write tests"));
        assert!(screen.contains("Details"));
    }

    #[test]
    fn selected_panel_wraps_long_report_and_output_lines_without_losing_latest_rows() {
        let pane = sample_pane("claude");
        let mut app = app_with_panes(
            vec![pane],
            vec![(
                "%1",
                vec![
                    "Waiting for leader approval to access internal deployment secrets and network routes for the release train",
                ],
            )],
        );
        set_pane_report_fields(
            &mut app,
            "%1",
            "waiting",
            "approval: network access for staging deploys and internal artifact mirrors",
            "approve request and resume the release validation pipeline immediately",
        );

        let lines = render_grid(&app, 120, 18);
        let screen = screen_text(&lines);
        let output_y = *line_indices(&lines, "Output")
            .last()
            .expect("output heading should be visible");
        let latest_line_y = line_index(&lines, "Waiting for leader approval to access internal");

        assert_render_invariants(&lines, 120, 18);
        assert!(!screen.contains("Agent report"), "{screen}");
        assert_eq!(output_y, line_index(&lines, "Queue: #1") + 2, "{screen}");
        assert_eq!(latest_line_y, output_y + 1, "{screen}");
        assert!(
            screen.contains("Blocked: network access for staging deploys and"),
            "{screen}"
        );
        assert!(screen.contains("internal artifact mirrors"), "{screen}");
        assert!(screen.contains("Action: : reply"), "{screen}");
        assert!(screen.contains("internal deployment"), "{screen}");
        assert!(
            screen.contains("secrets and network routes for the release train"),
            "{screen}"
        );
    }

    #[test]
    fn panel_line_wrapper_keeps_labels_and_indents_continuations() {
        assert_eq!(
            wrap_panel_line("Action: : reply", 32),
            vec![String::from("Action: : reply")]
        );
        assert_eq!(
            wrap_panel_line("  Type your answer to continue the deployment", 24),
            vec![
                String::from("  Type your answer to"),
                String::from("  continue the"),
                String::from("  deployment"),
            ]
        );
        assert_eq!(
            wrap_panel_line("Agent report", 5),
            vec![String::from("Ag...")]
        );
        assert_eq!(
            wrap_panel_line("Action: ", 20),
            vec![String::from("Action: ")]
        );
        assert_eq!(
            wrap_panel_line("Action: approve", 4),
            vec![
                String::from("Acti"),
                String::from("on:"),
                String::from("appr"),
                String::from("ove"),
            ]
        );
        assert_eq!(wrap_words("   ", 8), vec![String::new()]);
    }

    #[test]
    fn selected_details_keep_codex_activity_visible_when_next_is_long() {
        let pane = sample_pane("codex");
        let app = app_with_panes(
            vec![pane],
            vec![(
                "%1",
                vec![
                    "STATUS=running | BLOCKER=none | NEXT=approve deployment after smoke tests and restart worker pool in staging now",
                    "building renderer observability checks",
                ],
            )],
        );

        let lines = render_grid(&app, 100, 20);
        let screen = screen_text(&lines);
        let output_y = line_index(&lines, "Output");
        let activity_y = line_index(&lines, "build renderer observability checks");

        assert_render_invariants(&lines, 100, 20);
        assert!(screen.contains("Now: approve deployment after"), "{screen}");
        assert!(screen.contains("restart worker pool"), "{screen}");
        assert!(output_y > line_index(&lines, "Now:"), "{screen}");
        assert_eq!(activity_y, output_y + 1, "{screen}");
    }

    #[test]
    fn latest_and_next_use_fresh_runtime_report_over_stale_stored_report() {
        for width in [72, 100, 120] {
            let pane = sample_pane("codex");
            let mut app = app_with_panes(
                vec![pane],
                vec![(
                    "%1",
                    vec![
                        "STATUS=running | BLOCKER=none | NEXT=ship fix",
                        "building renderer observability checks",
                    ],
                )],
            );
            set_pane_report_fields(&mut app, "%1", "running", "none", "read docs");

            let lines = render_grid(&app, width, 20);
            let screen = screen_text(&lines);
            let board_row = lines
                .iter()
                .find(|line| line.contains("demo/agents") && line.contains("ship fix"))
                .unwrap_or_else(|| {
                    panic!("missing fresh Fleet Latest at width {width}:\n{screen}")
                });
            let next_line = lines
                .iter()
                .find(|line| line.contains("Now:"))
                .unwrap_or_else(|| panic!("missing Details Now at width {width}:\n{screen}"));

            assert_render_invariants(&lines, width, 20);
            assert!(
                board_row.contains("ship fix"),
                "Fleet Latest should show the fresh runtime NEXT at width {width}:\n{screen}"
            );
            assert!(
                next_line.contains("ship fix"),
                "Details Next should show the fresh runtime NEXT at width {width}:\n{screen}"
            );
            assert!(
                !screen.contains("read docs"),
                "stale stored report leaked into the UI at width {width}:\n{screen}"
            );
        }
    }

    #[test]
    fn inactive_panes_without_output_render_as_checking_not_unknown() {
        let mut pane = sample_pane("codex");
        pane.active = false;
        let app = app_with_panes(vec![pane], vec![]);
        let theme = Theme::from_preset(app.theme_preset());

        let buffer = render_buffer(&app, 96, 18);
        let lines = buffer_grid_lines(&buffer);
        let screen = screen_text(&lines);
        let state_value = buffer_cell(&buffer, &lines, "Checking");

        assert_render_invariants(&lines, 96, 18);
        assert!(screen.contains("State: Checking"), "{screen}");
        assert!(screen.contains("Action: G show in tmux"), "{screen}");
        assert!(screen.contains("checking"), "{screen}");
        assert_eq!(state_value.fg, theme.muted);
        assert!(screen.contains("codex: checking"), "{screen}");
        assert!(!screen.contains("codex: codex"), "{screen}");
        assert!(!screen.contains("codex codex"), "{screen}");
        assert!(!screen.contains("open in tmux"), "{screen}");
        assert!(!screen.contains("Unknown"), "{screen}");
        assert!(!screen.contains("unknown"), "{screen}");
        assert!(!screen.contains("unk"), "{screen}");
    }

    #[test]
    fn fleet_does_not_render_summary_template_placeholders() {
        let pane = sample_pane("node");
        let app = app_with_panes(
            vec![pane],
            vec![(
                "%1",
                vec![
                    "Reply in exactly one line as: STATUS=<status> | BLOCKER=<blocker> | NEXT=<next>.",
                    "NEXT=<next>.",
                    "building renderer tests",
                ],
            )],
        );

        let lines = render_grid(&app, 120, 18);
        let screen = screen_text(&lines);

        assert_render_invariants(&lines, 120, 18);
        assert!(!screen.contains("NEXT=<next>"), "{screen}");
        assert!(!screen.contains("<next>"), "{screen}");
        assert!(!screen.contains("idle NEXT"), "{screen}");
        assert!(screen.contains("build renderer tests"), "{screen}");
    }

    #[test]
    fn fleet_renders_user_intent_not_provider_scaffolding() {
        let pane = sample_pane("node");
        let app = app_with_panes(
            vec![pane],
            vec![(
                "%1",
                vec![
                    "Reply in exactly one line as: STATUS=<status> | BLOCKER=<blocker> |",
                    "NEXT=<next>.",
                    "gpt-5.4 high · ~/Projects/muxboard",
                    "› Run /review on my current changes",
                ],
            )],
        );

        let lines = render_grid(&app, 120, 18);
        let screen = screen_text(&lines);

        assert_render_invariants(&lines, 120, 18);
        assert!(screen.contains("codex review changes"), "{screen}");
        assert!(screen.contains("Now: review changes"), "{screen}");
        assert!(!screen.contains("<next>"), "{screen}");
        assert!(!screen.contains("gpt-5.4"), "{screen}");
    }

    #[test]
    fn usability_running_agents_show_current_work_not_fake_actions() {
        let pane = sample_pane("codex");
        let app = app_with_panes(
            vec![pane],
            vec![("%1", vec!["Running", "building release artifacts"])],
        );

        let lines = render_grid(&app, 100, 18);
        let screen = screen_text(&lines);

        assert_render_invariants(&lines, 100, 18);
        assert!(screen.contains("Now: build release artifacts"), "{screen}");
        assert!(!screen.contains("Action: continue work"), "{screen}");
        assert!(!screen.contains("Action: watch progress"), "{screen}");
        assert!(!screen.contains("Mission:"), "{screen}");
    }

    #[test]
    fn fleet_labels_node_harness_with_codex_hints_as_codex() {
        let pane = sample_pane("node");
        let app = app_with_panes(
            vec![pane],
            vec![(
                "%1",
                vec![
                    "gpt-5.4 high · ~/Projects/muxboard",
                    "STATUS=running | BLOCKER=none | NEXT=ship fix",
                ],
            )],
        );

        let lines = render_grid(&app, 120, 18);
        let screen = screen_text(&lines);

        assert_render_invariants(&lines, 120, 18);
        assert!(screen.contains("codex"), "{screen}");
        assert!(
            screen.contains("codex ship fix") || screen.contains("codex: ship fix"),
            "{screen}"
        );
        assert!(!screen.contains(" node "), "{screen}");
    }

    #[test]
    fn selected_fleet_latest_wraps_to_three_lines_without_hiding_details() {
        let pane = sample_pane("codex");
        let app = app_with_panes(
            vec![pane],
            vec![(
                "%1",
                vec![
                    "STATUS=running | BLOCKER=none | NEXT=evaluate deployment rollout after staging smoke tests restart worker pool confirm regional health checks compare canary metrics notify release captain and prepare rollback notes",
                    "building renderer observability checks",
                ],
            )],
        );

        let lines = render_grid(&app, 120, 20);
        let screen = screen_text(&lines);
        let first_latest_y = line_index(&lines, "codex evaluate deployment rollout");

        assert_render_invariants(&lines, 120, 20);
        assert!(lines[first_latest_y].contains(">+ demo/agents"), "{screen}");
        assert!(
            lines[first_latest_y + 1].contains("smoke tests restart worker"),
            "{screen}"
        );
        assert!(
            lines[first_latest_y + 2].contains("regional")
                || lines[first_latest_y + 2].contains("..."),
            "{screen}"
        );
        assert!(screen.contains("Details"), "{screen}");
        assert!(
            screen.contains("Now: evaluate deployment rollout"),
            "{screen}"
        );
    }

    #[test]
    fn selected_latest_wrapper_caps_at_three_lines_with_visible_ellipsis() {
        let row = BoardRow {
            selected: true,
            active: true,
            marked: false,
            targeted: false,
            staged: false,
            show_command_in_latest: false,
            attention: String::new(),
            status: String::from("running"),
            lifecycle: String::from("working"),
            mission: String::new(),
            heat: String::new(),
            age: String::new(),
            cpu: String::new(),
            mem: String::new(),
            lane: String::new(),
            pane: String::from("%1"),
            location: String::from("demo/agents"),
            command: String::from("codex"),
            title: String::from(""),
        };

        let lines = selected_board_latest_lines(
            &row,
            String::from(
                "one two three four five six seven eight nine ten eleven twelve thirteen fourteen",
            ),
            18,
        );

        assert_eq!(lines.len(), 3);
        assert_eq!(lines[2], "eight nine ten...");
        assert!(lines[2].ends_with("..."), "{lines:?}");
    }

    #[test]
    fn selected_latest_wrapper_handles_tiny_widths_without_overflow() {
        let row = BoardRow {
            selected: true,
            active: true,
            marked: false,
            targeted: false,
            staged: false,
            show_command_in_latest: false,
            attention: String::new(),
            status: String::from("running"),
            lifecycle: String::from("working"),
            mission: String::new(),
            heat: String::new(),
            age: String::new(),
            cpu: String::new(),
            mem: String::new(),
            lane: String::new(),
            pane: String::from("%1"),
            location: String::from("demo/agents"),
            command: String::from("codex"),
            title: String::new(),
        };

        assert_eq!(append_cell_ellipsis("abcdef", 0), "");
        assert_eq!(append_cell_ellipsis("abcdef", 2), "ab");

        let lines = selected_board_latest_lines(
            &row,
            String::from("alpha beta gamma delta epsilon zeta"),
            3,
        );

        assert_eq!(lines.len(), 3);
        assert!(
            lines.iter().all(|line| line.chars().count() <= 3),
            "{lines:?}"
        );
        assert!(lines.iter().all(|line| !line.contains("...")), "{lines:?}");
    }

    #[test]
    fn usability_selected_latest_wrapper_packs_hidden_words_before_ellipsis() {
        let row = BoardRow {
            selected: true,
            active: true,
            marked: false,
            targeted: false,
            staged: false,
            show_command_in_latest: false,
            attention: String::new(),
            status: String::from("running"),
            lifecycle: String::from("working"),
            mission: String::new(),
            heat: String::new(),
            age: String::new(),
            cpu: String::new(),
            mem: String::new(),
            lane: String::new(),
            pane: String::from("%1"),
            location: String::from("demo/agents"),
            command: String::from("codex"),
            title: String::new(),
        };

        let lines = selected_board_latest_lines(
            &row,
            String::from("alpha beta gamma delta epsilon zeta eta theta iota kappa lambda"),
            20,
        );

        assert_eq!(
            lines,
            vec![
                String::from("alpha beta gamma"),
                String::from("delta epsilon zeta"),
                String::from("eta theta iota ka...")
            ]
        );
    }

    #[test]
    fn output_panel_wraps_long_tail_lines_before_they_push_down_following_output() {
        let pane = sample_pane("bash");
        let app = app_with_panes(
            vec![pane],
            vec![(
                "%1",
                vec![
                    "tool bash running an extremely long validation sequence across region-us-west-2 and eu-central-1 with multiple follow-up checks",
                    "second tail marker",
                ],
            )],
        );
        let mut app = app;
        app.cycle_context_pane();

        let lines = render_grid(&app, 68, 14);
        let screen = screen_text(&lines);
        let summary_y = line_index(&lines, "Summary");
        let latest_y = line_index(&lines, "Latest");
        let first_tail_y = line_index(
            &lines,
            "tool bash running an extremely long validation sequence",
        );
        let second_tail_matches = line_indices(&lines, "second tail marker");

        assert_render_invariants(&lines, 68, 14);
        assert_eq!(latest_y, summary_y + 2, "{screen}");
        assert_eq!(first_tail_y, latest_y + 1, "{screen}");
        assert!(
            screen.contains("across region-us-west-2 and eu-central-1"),
            "{screen}"
        );
        assert!(screen.contains("follow-up checks"), "{screen}");
        assert_eq!(second_tail_matches.len(), 1, "{screen}");
        assert_eq!(second_tail_matches[0], summary_y + 1, "{screen}");
    }

    #[test]
    fn short_selected_panel_keeps_identity_queue_and_output_before_seen_lines() {
        let pane = sample_pane("claude");
        let mut app = app_with_panes(
            vec![pane],
            vec![(
                "%1",
                vec![
                    "Waiting for leader approval to access internal deployment secrets and network routes for the release train",
                    "approval remains blocked on staging network access",
                ],
            )],
        );
        set_pane_report_fields(
            &mut app,
            "%1",
            "waiting",
            "approval: network access for staging deploys and internal artifact mirrors",
            "approve request and resume the release validation pipeline immediately",
        );

        let raw_lines = app.inspector_lines();
        let expected = prepare_context_panel_lines("Details", raw_lines, 47, 10);
        let lines = render_grid(&app, 109, 14);
        let screen = screen_text(&lines);

        assert_render_invariants(&lines, 109, 14);
        assert!(expected.contains(&String::from("Output")));
        for line in [
            "demo/agents",
            "State: Waiting   Tool: Claude Code",
            "Blocked: network access for staging deploys and",
            "         internal artifact mirrors",
            "Action: : reply",
            "Output",
            "  Waiting for leader approval to access internal",
            "  deployment secrets and network routes for the release",
            "  train",
            "  approval remains blocked on staging network access",
        ] {
            assert!(screen.contains(line), "{screen}");
        }
        assert!(!screen.contains("Activity:"), "{screen}");
        let selected_output_y = *line_indices(&lines, "Output")
            .last()
            .expect("selected output heading should be visible");
        assert!(
            line_index(&lines, "Action:") < selected_output_y,
            "{screen}"
        );
        assert!(
            !screen.contains("Updated: 0s ago | awaiting input"),
            "{screen}"
        );
    }

    #[test]
    fn medium_width_stacks_to_preserve_latest_and_details_next() {
        let pane = sample_pane("claude");
        let mut app = app_with_panes(
            vec![pane],
            vec![(
                "%1",
                vec![
                    "waiting for release approval after staging smoke tests",
                    "restart worker pool after approval",
                ],
            )],
        );
        set_pane_report_fields(
            &mut app,
            "%1",
            "waiting",
            "none",
            "approve deployment after smoke tests and restart worker pool",
        );

        let lines = render_grid(&app, 100, 20);
        let screen = screen_text(&lines);
        let fleet_y = line_index(&lines, "Fleet");
        let details_y = line_index(&lines, "Details");
        let action_line = lines
            .iter()
            .find(|line| line.contains("Action:"))
            .unwrap_or_else(|| panic!("missing Action line:\n{screen}"));

        assert_render_invariants(&lines, 100, 20);
        assert!(details_y > fleet_y, "{screen}");
        assert!(
            screen.contains("restart worker pool"),
            "Latest should preserve the important tail at medium width:\n{screen}"
        );
        assert!(action_line.contains(": reply"), "{screen}");
        assert!(
            !action_line.contains("restart worker pool"),
            "waiting reply actions should stay short; the blocker and output carry the context:\n{screen}"
        );
    }

    #[test]
    fn short_output_overlay_spends_scarce_rows_on_latest_before_summary() {
        let pane = sample_pane("bash");
        let mut app = app_with_panes(
            vec![pane],
            vec![(
                "%1",
                vec![
                    "step one preparing release image",
                    "step two syncing artifacts",
                    "step three validating checksums",
                    "step four notifying staging",
                    "step five completed handoff",
                ],
            )],
        );
        app.cycle_context_pane();

        let (_, raw_lines) = app
            .overlay_panel()
            .expect("output overlay should be visible");
        let body = body_rect(109, 14);
        let overlay = overlay_rect(body, "Output", &raw_lines);
        let expected = prepare_overlay_lines(
            "Output",
            raw_lines,
            overlay.width.saturating_sub(4),
            overlay.height,
        );
        let lines = render_grid(&app, 109, 14);
        let screen = screen_text(&lines);

        assert_render_invariants(&lines, 109, 14);
        assert_eq!(
            expected,
            vec![
                String::from("demo / agents"),
                String::from("Running | 0s ago"),
                String::from("Latest"),
                String::from("  step one preparing release image"),
                String::from("  step two syncing artifacts"),
                String::from("  step three validating checksums"),
                String::from("  step four notifying staging"),
                String::from("  complete handoff"),
            ]
        );
        for line in &expected {
            assert!(screen.contains(line), "{screen}");
        }
        assert!(!screen.contains("Summary"), "{screen}");
        assert!(line_index(&lines, "Latest") < line_index(&lines, "complete handoff"));
    }

    #[test]
    fn tiny_output_overlay_keeps_latest_and_recovery_before_summary() {
        let pane = sample_pane("bash");
        let mut app = app_with_panes(
            vec![pane],
            vec![(
                "%1",
                vec![
                    "step one preparing release image",
                    "step two syncing artifacts",
                    "step three validating checksums",
                    "step four notifying staging",
                    "step five completed handoff",
                ],
            )],
        );
        app.cycle_context_pane();

        let lines = render_grid(&app, 60, 12);
        let screen = screen_text(&lines);

        assert_render_invariants(&lines, 60, 12);
        assert!(screen.contains("Output"), "{screen}");
        assert!(screen.contains("Latest"), "{screen}");
        assert!(screen.contains("complete handoff"), "{screen}");
        assert!(screen.contains("step four notifying staging"), "{screen}");
        assert!(screen.contains("Esc back"), "{screen}");
        assert!(
            !screen.contains("Summary"),
            "scarce Output space should go to visible output before summary headings:\n{screen}"
        );
        assert!(
            !screen.contains("step one preparing release image"),
            "{screen}"
        );
    }

    #[test]
    fn help_overlay_stays_small_and_keeps_core_actions_visible() {
        let mut app = app_from_view_model_fixture(&view_fixture("attention_board_row_summary"));
        app.toggle_help_overlay();
        let lines = render_lines(&app, 96, 18);
        let screen = screen_text(&lines);

        assert_no_garbled_text(&lines);
        assert!(screen.contains("Help"), "{screen}");
        assert!(
            screen.contains("Now: : reply, Enter output, G show in tmux."),
            "{screen}"
        );
        assert!(
            screen.contains("Send: Space add/remove pane for a send list."),
            "{screen}"
        );
        assert!(
            screen.contains("Move: J/K select panes, Tab Fleet/Details."),
            "{screen}"
        );
        assert!(
            screen.contains("Views: . then [ browse, ] command center; L layout."),
            "{screen}"
        );
        assert!(
            screen.contains("More: . then + start agent, Z zoom pane."),
            "{screen}"
        );
        assert!(screen.contains("Legend: > selected"), "{screen}");
    }

    #[test]
    fn help_overlay_matches_secondary_surface_actions() {
        let pane = sample_pane("codex");
        let mut browse = app_with_panes(vec![pane.clone()], vec![]);
        browse.show_browse_view();
        browse.toggle_help_overlay();
        let browse_lines = render_grid(&browse, 96, 18);
        let browse_screen = screen_text(&browse_lines);

        assert_render_invariants(&browse_lines, 96, 18);
        assert!(
            browse_screen.contains("Now: Enter opens window, Esc back, G show in tmux."),
            "{browse_screen}"
        );
        assert!(
            browse_screen.contains("Move: J/K browse windows."),
            "{browse_screen}"
        );
        assert!(!browse_screen.contains("Fleet/Details"), "{browse_screen}");

        let mut empty_browse = app_from_panel_fixture(&panel_fixture("navigator_empty_state"));
        empty_browse.toggle_help_overlay();
        let empty_browse_lines = render_grid(&empty_browse, 96, 18);
        let empty_browse_screen = screen_text(&empty_browse_lines);

        assert_render_invariants(&empty_browse_lines, 96, 18);
        assert!(
            empty_browse_screen.contains("Now: backspace shows all panes, Esc back."),
            "{empty_browse_screen}"
        );
        for inert in ["Enter opens window", "G show in tmux", "J/K browse windows"] {
            assert!(
                !empty_browse_screen.contains(inert),
                "empty Browse Help advertised inert `{inert}`:\n{empty_browse_screen}"
            );
        }

        let mut no_pane_browse = app_with_panes(Vec::new(), vec![]);
        no_pane_browse.show_browse_view();
        no_pane_browse.toggle_help_overlay();
        let no_pane_browse_lines = render_grid(&no_pane_browse, 96, 18);
        let no_pane_browse_screen = screen_text(&no_pane_browse_lines);

        assert_render_invariants(&no_pane_browse_lines, 96, 18);
        assert!(
            no_pane_browse_screen.contains("Now: R refreshes panes, Esc back."),
            "{no_pane_browse_screen}"
        );
        assert!(
            no_pane_browse_screen.contains("Find: / filter, R refresh."),
            "{no_pane_browse_screen}"
        );
        for inert in [
            "Enter opens window",
            "G show in tmux",
            "J/K browse windows",
            "backspace show all",
        ] {
            assert!(
                !no_pane_browse_screen.contains(inert),
                "no-pane Browse Help advertised inert `{inert}`:\n{no_pane_browse_screen}"
            );
        }

        let mut command_center = app_with_panes(vec![pane], vec![]);
        command_center.show_command_center();
        command_center.toggle_help_overlay();
        let command_lines = render_grid(&command_center, 96, 18);
        let command_screen = screen_text(&command_lines);

        assert_render_invariants(&command_lines, 96, 18);
        assert!(
            command_screen.contains("Now: Enter output, Esc back, G show in tmux."),
            "{command_screen}"
        );
        assert!(
            command_screen.contains("Move: J/K choose action."),
            "{command_screen}"
        );
        assert!(
            !command_screen.contains("Fleet/Details"),
            "{command_screen}"
        );
    }

    #[test]
    fn help_overlay_empty_states_show_recovery_not_inert_pane_actions() {
        let mut empty = app_with_panes(Vec::new(), vec![]);
        empty.toggle_help_overlay();
        let empty_lines = render_grid(&empty, 96, 18);
        let empty_screen = screen_text(&empty_lines);

        assert_render_invariants(&empty_lines, 96, 18);
        assert_screen_has_one_line_chrome("empty Help", &empty_lines);
        assert!(
            empty_screen.contains("Now: start tmux panes, then R refresh."),
            "{empty_screen}"
        );
        assert!(
            empty_screen.contains("More: . layout and settings."),
            "{empty_screen}"
        );
        for inert in [
            "Enter output",
            "G show in tmux",
            ": command pane",
            "Space add/remove pane",
            "+ start agent",
            "Z zoom pane",
        ] {
            assert!(
                !empty_screen.contains(inert),
                "empty Help advertised inert `{inert}`:\n{empty_screen}"
            );
        }

        let mut no_match = app_with_panes(vec![sample_pane("codex")], vec![]);
        no_match.set_search_query_for_test("zz-no-match");
        no_match.toggle_help_overlay();
        let no_match_lines = render_grid(&no_match, 96, 18);
        let no_match_screen = screen_text(&no_match_lines);

        assert_render_invariants(&no_match_lines, 96, 18);
        assert_screen_has_one_line_chrome("no-match Help", &no_match_lines);
        assert!(
            no_match_screen.contains("Now: backspace show all panes."),
            "{no_match_screen}"
        );
        assert!(
            no_match_screen.contains("Find: / filter, R refresh."),
            "{no_match_screen}"
        );
        for inert in [
            "Enter output",
            "G show in tmux",
            ": command pane",
            "Space add/remove pane",
            "+ start agent",
            "Z zoom pane",
        ] {
            assert!(
                !no_match_screen.contains(inert),
                "no-match Help advertised inert `{inert}`:\n{no_match_screen}"
            );
        }
    }

    #[test]
    fn help_overlay_only_shows_continue_when_selected_pane_is_enter_safe() {
        let mut idle = app_with_panes(vec![sample_pane("codex")], vec![]);
        idle.toggle_help_overlay();
        let idle_lines = render_grid(&idle, 96, 18);
        let idle_screen = screen_text(&idle_lines);

        assert_render_invariants(&idle_lines, 96, 18);
        assert!(
            idle_screen.contains("Now: Enter output, G show in tmux."),
            "{idle_screen}"
        );
        assert!(!idle_screen.contains("A continue waiting"), "{idle_screen}");

        let mut waiting = app_with_panes(
            vec![sample_pane("claude")],
            vec![("%1", vec!["Press Enter to continue."])],
        );
        waiting.toggle_help_overlay();
        let waiting_lines = render_grid(&waiting, 96, 18);
        let waiting_screen = screen_text(&waiting_lines);

        assert_render_invariants(&waiting_lines, 96, 18);
        assert!(
            waiting_screen.contains("Now: Enter output, G show in tmux, A continue waiting."),
            "{waiting_screen}"
        );

        let mut choice = app_with_panes(
            vec![sample_pane("claude")],
            vec![("%1", vec!["Allow command? [y/n]"])],
        );
        choice.toggle_help_overlay();
        let choice_lines = render_grid(&choice, 96, 18);
        let choice_screen = screen_text(&choice_lines);

        assert_render_invariants(&choice_lines, 96, 18);
        assert!(
            choice_screen.contains("Now: . answer yes/no, : send, G show in tmux."),
            "{choice_screen}"
        );
        assert!(
            !choice_screen.contains("A continue waiting"),
            "{choice_screen}"
        );
    }

    #[test]
    fn narrow_help_overlay_wraps_sentences_in_place_without_extra_rows() {
        let mut app = app_from_view_model_fixture(&view_fixture("attention_board_row_summary"));
        app.toggle_help_overlay();
        let raw_lines = app.help_lines();
        let body = body_rect(68, 14);
        let overlay = overlay_rect(body, &app.help_overlay_title(), &raw_lines);
        let inner_width = overlay.width.saturating_sub(4);
        let expected = raw_lines
            .iter()
            .flat_map(|line| wrap_panel_line(line, inner_width))
            .collect::<Vec<_>>();

        let lines = render_grid(&app, 68, 14);
        let screen = screen_text(&lines);
        let first_y = line_index(&lines, &expected[0]);

        assert_render_invariants(&lines, 68, 14);
        for (offset, line) in expected.iter().enumerate() {
            assert_eq!(line_indices(&lines, line).len(), 1, "{screen}");
            assert_eq!(line_index(&lines, line), first_y + offset, "{screen}");
        }
    }

    #[test]
    fn tiny_help_overlay_keeps_core_tasks_and_recovery_visible() {
        let mut app = app_from_view_model_fixture(&view_fixture("attention_board_row_summary"));
        app.toggle_help_overlay();

        let lines = render_grid(&app, 60, 12);
        let screen = screen_text(&lines);

        assert_render_invariants(&lines, 60, 12);
        assert!(screen.contains("Help"), "{screen}");
        assert!(screen.contains("Now:"), "{screen}");
        assert!(screen.contains("Send:"), "{screen}");
        assert!(screen.contains("Move:"), "{screen}");
        assert!(screen.contains("Esc close"), "{screen}");
    }

    #[test]
    fn narrow_actions_overlay_truncates_long_lines_in_place_and_keeps_sections_dense() {
        let app = app_from_panel_fixture(&panel_fixture("actions_menu_sections"));
        let (_, raw_lines) = app
            .overlay_panel()
            .expect("actions overlay should be visible");
        let body = body_rect(80, 20);
        let overlay = overlay_rect(body, "More", &raw_lines);
        let inner_width = overlay.width.saturating_sub(4);
        let expected_next = truncate_panel_line(&raw_lines[0], inner_width);
        let expected_target = truncate_panel_line(&raw_lines[1], inner_width);
        let expected_first_view = truncate_panel_line("  : send text", inner_width);
        let expected_first_pane = truncate_panel_line("  Space add to send list", inner_width);
        let expected_zoom = truncate_panel_line("  Z zoom pane", inner_width);
        let expected_launch = truncate_panel_line("  + start agent", inner_width);

        let lines = render_grid(&app, 80, 20);
        let screen = screen_text(&lines);
        let next_y = line_index(&lines, &expected_next);
        let target_y = line_index(&lines, &expected_target);
        let view_y = line_index(&lines, "View");
        let first_view_y = line_index(&lines, &expected_first_view);
        let pane_y = line_index(&lines, "│ Pane");
        let first_pane_y = line_index(&lines, &expected_first_pane);
        let zoom_y = line_index(&lines, &expected_zoom);
        let start_y = line_index(&lines, "│ Start");
        let launch_y = line_index(&lines, &expected_launch);

        assert_render_invariants(&lines, 80, 20);
        assert_eq!(target_y, next_y + 1, "{screen}");
        assert_eq!(view_y, target_y + 1, "{screen}");
        assert_eq!(first_view_y, view_y + 1, "{screen}");
        assert!(start_y > first_view_y, "{screen}");
        assert_eq!(start_y, first_view_y + 5, "{screen}");
        assert_eq!(launch_y, start_y + 1, "{screen}");
        assert!(pane_y > launch_y, "{screen}");
        assert_eq!(pane_y, launch_y + 1, "{screen}");
        assert_eq!(first_pane_y, pane_y + 1, "{screen}");
        assert_eq!(zoom_y, first_pane_y + 1, "{screen}");
        assert_eq!(line_indices(&lines, &expected_next).len(), 1, "{screen}");
        assert_eq!(
            line_indices(&lines, &expected_first_view).len(),
            1,
            "{screen}"
        );
        assert_eq!(
            line_indices(&lines, &expected_first_pane).len(),
            1,
            "{screen}"
        );
    }

    #[test]
    fn actions_overlay_matches_browse_enter_behavior() {
        let pane = sample_pane("codex");
        let mut app = app_with_panes(vec![pane], vec![]);
        app.show_browse_view();
        app.open_action_menu();
        let lines = render_grid(&app, 96, 18);
        let screen = screen_text(&lines);

        assert_render_invariants(&lines, 96, 18);
        assert!(screen.contains("More"), "{screen}");
        assert!(screen.contains("Enter open window"), "{screen}");
        assert!(
            !screen.contains("Enter show output"),
            "Browse More overlay should not advertise the normal Details action:\n{screen}"
        );
        assert!(screen.contains("[ browse windows"), "{screen}");
        assert!(
            screen.contains("? help  press a listed key  Esc close"),
            "{screen}"
        );
    }

    #[test]
    fn short_more_overlay_prioritizes_view_and_pane_sections_before_lower_value_blocks() {
        let app = app_from_panel_fixture(&panel_fixture("actions_menu_sections"));
        let (_, raw_lines) = app
            .overlay_panel()
            .expect("actions overlay should be visible");
        let body = body_rect(80, 16);
        let overlay = overlay_rect(body, "More", &raw_lines);
        let expected = prepare_overlay_lines(
            "More",
            raw_lines.clone(),
            overlay.width.saturating_sub(4),
            overlay.height,
        );
        let lines = render_grid(&app, 80, 16);
        let screen = screen_text(&lines);

        assert_render_invariants(&lines, 80, 16);
        assert!(expected.contains(&String::from("View")));
        assert!(expected.contains(&String::from("Pane")));
        assert!(!expected.contains(&String::from("targeting")));
        assert!(screen.contains("View"), "{screen}");
        assert!(screen.contains("Pane"), "{screen}");
        assert!(screen.contains("Space add to send list"), "{screen}");
        assert!(
            !screen.contains("C mute alert"),
            "irrelevant alert actions should not outrank useful pane actions:\n{screen}"
        );
        assert!(!screen.contains("X clear send list"), "{screen}");
    }

    #[test]
    fn short_more_overlay_keeps_active_fleet_clear_visible() {
        let mut first = sample_pane("codex");
        first.id = String::from("%1");
        first.window_id = String::from("@1");
        first.window_name = String::from("prompt");

        let mut second = sample_pane("claude");
        second.id = String::from("%2");
        second.window_id = String::from("@2");
        second.window_name = String::from("prompt2");
        second.pane_index = 1;
        second.active = false;

        let mut app = app_with_panes(vec![first, second], vec![]);
        app.set_target_groups_for_test(vec![crate::app::TargetGroup {
            name: String::from("triage"),
            members: vec![
                crate::app::PaneLocator {
                    session_name: String::from("demo"),
                    window_name: String::from("prompt"),
                    pane_index: 0,
                },
                crate::app::PaneLocator {
                    session_name: String::from("demo"),
                    window_name: String::from("prompt2"),
                    pane_index: 1,
                },
            ],
        }]);
        app.load_next_target_group();
        app.open_action_menu();

        let lines = render_grid(&app, 80, 20);
        let screen = screen_text(&lines);

        assert_render_invariants(&lines, 80, 20);
        assert!(screen.contains("More"), "{screen}");
        assert!(screen.contains("fleet triage"), "{screen}");
        assert!(
            screen.contains("X clear send list"),
            "active fleets must keep the visible clear action above the fold:\n{screen}"
        );
        assert!(
            screen.contains("G save fleet"),
            "active fleets should keep save visible beside clear when space is tight:\n{screen}"
        );
    }

    #[test]
    fn standard_more_overlay_keeps_summary_and_settings_visible_without_spacer_waste() {
        let app = app_from_panel_fixture(&panel_fixture("actions_menu_sections"));
        let lines = render_grid(&app, 80, 24);
        let screen = screen_text(&lines);

        assert_render_invariants(&lines, 80, 24);
        for term in [
            "Action: : send this pane",
            "View",
            "  Enter show output",
            "Pane",
            "  S summarize panes",
            "  Z zoom pane",
            "Settings",
            "  M pane CPU/mem",
            "  O desktop alerts",
            "  V terminal bell",
        ] {
            assert!(screen.contains(term), "More missing `{term}`:\n{screen}");
        }
        assert!(
            !screen.contains("  L layout: auto") || screen.contains("  S summarize panes"),
            "layout must never displace the higher-value summary action on a normal terminal:\n{screen}"
        );
        assert!(
            line_index(&lines, "Settings") > line_index(&lines, "Pane"),
            "settings should sit below the core pane actions, not crowd the primary path:\n{screen}"
        );
        assert!(
            line_index(&lines, "Settings") < 20,
            "settings should be discoverable on a normal terminal, not below the fold:\n{screen}"
        );
    }

    #[test]
    fn common_more_overlay_keeps_send_list_save_fleet_visible() {
        let mut first = sample_pane("codex");
        first.id = String::from("%1");
        let mut second = sample_pane("claude");
        second.id = String::from("%2");
        second.active = false;
        second.pane_index = 1;

        let mut app = app_with_panes(vec![first, second], vec![]);
        app.toggle_selected_mark();
        app.select_next_pane();
        app.toggle_selected_mark();
        app.open_action_menu();

        let lines = render_grid(&app, 80, 20);
        let screen = screen_text(&lines);

        assert_render_invariants(&lines, 80, 20);
        assert!(screen.contains("More"), "{screen}");
        assert!(screen.contains("send list (2 panes)"), "{screen}");
        assert!(screen.contains("send list"), "{screen}");
        assert!(
            screen.contains("G save fleet"),
            "More promised listed keys while hiding the reusable-fleet action:\n{screen}"
        );
        assert!(
            !screen.contains("press a listed key") || screen.contains("G save fleet"),
            "footer promised only listed keys while the save action was hidden:\n{screen}"
        );
    }

    #[test]
    fn tiny_more_overlay_keeps_start_agent_discoverable() {
        let app = app_from_panel_fixture(&panel_fixture("actions_menu_sections"));
        let lines = render_grid(&app, 60, 12);
        let screen = screen_text(&lines);

        assert_render_invariants(&lines, 60, 12);
        assert!(screen.contains("More"), "{screen}");
        assert!(
            screen.contains("+ start agent"),
            "tiny More overlay hid the first-run start path:\n{screen}"
        );
        assert!(
            screen.contains("] command center"),
            "tiny More overlay hid the conductor-level command center:\n{screen}"
        );
        assert!(
            !screen.contains("press a listed key") || screen.contains("] command center"),
            "tiny More promised only listed keys while hiding Command Center:\n{screen}"
        );
    }

    #[test]
    fn tiny_narrowed_more_overlay_keeps_show_all_recovery_visible() {
        let mut first = sample_pane("codex");
        first.id = String::from("%1");
        first.window_name = String::from("alpha");
        let mut second = sample_pane("claude");
        second.id = String::from("%2");
        second.window_id = String::from("@2");
        second.window_name = String::from("beta");
        second.active = false;
        second.pane_index = 1;

        let mut app = app_with_panes(vec![first, second], vec![]);
        app.set_search_query_for_test("codex");
        app.open_action_menu();

        let lines = render_grid(&app, 60, 12);
        let screen = screen_text(&lines);

        assert_render_invariants(&lines, 60, 12);
        assert!(screen.contains("More"), "{screen}");
        assert!(
            screen.contains("backspace show all panes"),
            "tiny narrowed More must keep the obvious recovery action visible:\n{screen}"
        );
    }

    #[test]
    fn usability_more_top_recommendations_are_visible_listed_actions() {
        fn assert_recommendations_are_listed(name: &str, app: &App, width: u16, height: u16) {
            let lines = render_grid(app, width, height);
            let screen = screen_text(&lines);
            let recommendation = lines
                .iter()
                .find_map(|line| line.split_once("Action: ").map(|(_, tail)| tail.trim()))
                .unwrap_or_else(|| panic!("{name} rendered no More recommendation:\n{screen}"));
            let keys = recommendation
                .split(',')
                .filter_map(|part| {
                    let part = part.trim().strip_prefix("or ").unwrap_or(part.trim());
                    part.split_whitespace().next()
                })
                .collect::<Vec<_>>();
            let listed_rows = lines
                .iter()
                .filter(|line| !line.contains("Action: "))
                .cloned()
                .collect::<Vec<_>>()
                .join("\n");

            assert_render_invariants(&lines, width, height);
            for key in keys {
                assert!(
                    listed_rows.contains(&format!("{key} ")),
                    "{name} recommended `{key}` without listing it at {width}x{height}:\n{screen}"
                );
            }
        }

        let mut normal = app_from_panel_fixture(&panel_fixture("actions_menu_sections"));
        normal.open_action_menu();
        assert_recommendations_are_listed("normal More", &normal, 60, 12);
        assert_recommendations_are_listed("normal More", &normal, 80, 24);

        let mut empty = app_with_panes(Vec::new(), vec![]);
        empty.open_action_menu();
        assert_recommendations_are_listed("empty More", &empty, 60, 12);
        assert_recommendations_are_listed("empty More", &empty, 80, 16);

        let mut no_match = app_with_panes(vec![sample_pane("codex")], vec![]);
        no_match.set_search_query_for_test("zz-no-match");
        no_match.open_action_menu();
        assert_recommendations_are_listed("no-match More", &no_match, 60, 12);
        assert_recommendations_are_listed("no-match More", &no_match, 80, 16);
    }

    #[test]
    fn empty_more_overlay_is_recovery_not_action_dump() {
        let mut empty = app_with_panes(Vec::new(), vec![]);
        empty.open_action_menu();
        let lines = render_grid(&empty, 80, 16);
        let screen = screen_text(&lines);

        assert_render_invariants(&lines, 80, 16);
        assert!(screen.contains("More"), "{screen}");
        assert!(
            screen.contains("Action: R refresh after starting tmux panes"),
            "{screen}"
        );
        assert!(
            screen.contains("start tmux panes, then refresh"),
            "{screen}"
        );
        for inert in [
            "send to selected pane",
            ": send text",
            "S summarize panes",
            "+ start agent",
            "Enter show output",
        ] {
            assert!(
                !screen.contains(inert),
                "empty More advertised inert `{inert}`:\n{screen}"
            );
        }

        let mut no_match = app_with_panes(vec![sample_pane("codex")], vec![]);
        no_match.set_search_query_for_test("zz-no-match");
        no_match.open_action_menu();
        let no_match_lines = render_grid(&no_match, 80, 16);
        let no_match_screen = screen_text(&no_match_lines);

        assert_render_invariants(&no_match_lines, 80, 16);
        assert!(no_match_screen.contains("More"), "{no_match_screen}");
        assert!(
            no_match_screen.contains("Action: backspace show all panes"),
            "{no_match_screen}"
        );
        assert!(
            no_match_screen.contains("backspace show all panes"),
            "{no_match_screen}"
        );
        for inert in [": send text", "S summarize panes", "Enter show output"] {
            assert!(
                !no_match_screen.contains(inert),
                "no-match More advertised inert `{inert}`:\n{no_match_screen}"
            );
        }
    }

    #[test]
    fn short_send_overlay_keeps_target_identity_and_confirm_confirmation_before_history_noise() {
        let fixture = ViewModelFixture {
            name: String::from("short_confirm_send"),
            panes: vec![
                crate::app::tests::ViewModelPaneFixture {
                    id: String::from("%1"),
                    command: String::from("codex"),
                    window_name: String::from("agents"),
                    active: true,
                },
                crate::app::tests::ViewModelPaneFixture {
                    id: String::from("%2"),
                    command: String::from("codex"),
                    window_name: String::from("agents"),
                    active: false,
                },
                crate::app::tests::ViewModelPaneFixture {
                    id: String::from("%3"),
                    command: String::from("codex"),
                    window_name: String::from("agents"),
                    active: false,
                },
                crate::app::tests::ViewModelPaneFixture {
                    id: String::from("%4"),
                    command: String::from("codex"),
                    window_name: String::from("agents"),
                    active: false,
                },
                crate::app::tests::ViewModelPaneFixture {
                    id: String::from("%5"),
                    command: String::from("claude"),
                    window_name: String::from("review"),
                    active: false,
                },
            ],
            runtimes: Vec::new(),
            search_query: String::new(),
            marked_pane_ids: vec![
                String::from("%1"),
                String::from("%2"),
                String::from("%3"),
                String::from("%4"),
                String::from("%5"),
            ],
            metrics_mode: String::new(),
            command_input: String::new(),
            pending_dispatch: Some(crate::app::tests::ViewModelDispatchFixture {
                text: String::from("continue"),
                expanded: vec![
                    (String::from("%1"), String::from("continue")),
                    (String::from("%2"), String::from("continue")),
                    (String::from("%3"), String::from("continue")),
                    (String::from("%4"), String::from("continue")),
                    (String::from("%5"), String::from("continue")),
                ],
                target_description: String::from("fleet triage (5 panes)"),
            }),
            board_title_limit: None,
            board_title_width: None,
            header_width: None,
            footer_width: None,
            board_row_limit: None,
            expected_board_title: None,
            expected_header_context: None,
            expected_footer: None,
            expected_first_row: None,
        };
        let mut app = app_from_view_model_fixture(&fixture);
        remember_command_for_test(&mut app, "cargo test");

        let (_, raw_lines) = app.overlay_panel().expect("send overlay should be visible");
        let body = body_rect(80, 14);
        let overlay = overlay_rect(body, "Send", &raw_lines);
        let expected = prepare_overlay_lines(
            "Send",
            raw_lines.clone(),
            overlay.width.saturating_sub(4),
            overlay.height,
        );
        let lines = render_grid(&app, 80, 14);
        let screen = screen_text(&lines);

        assert_render_invariants(&lines, 80, 14);
        assert_eq!(
            expected,
            vec![
                String::from("To: fleet triage (5 panes)"),
                String::from("Text: continue"),
                String::from("Targets"),
                String::from("  demo / agents #0 continue"),
                String::from("  demo / agents #1 continue"),
                String::from("  ... 3 more"),
            ]
        );
        for line in &expected {
            if !line.is_empty() {
                assert!(screen.contains(line), "{screen}");
            }
        }
        assert_eq!(
            line_index(&lines, "To: fleet triage (5 panes)") + 1,
            line_index(&lines, "Text: continue"),
            "{screen}"
        );
        assert_eq!(
            line_index(&lines, "Targets") + 1,
            line_index(&lines, "  demo / agents #0 continue"),
            "{screen}"
        );
        assert!(
            line_index(&lines, "To: fleet triage (5 panes)") < line_index(&lines, "Targets"),
            "{screen}"
        );
        assert!(!screen.contains("send to send list"), "{screen}");
        assert!(!screen.contains("send to list"), "{screen}");
        assert!(
            line_index(&lines, "Text: continue")
                < line_index(&lines, "  demo / agents #0 continue"),
            "{screen}"
        );
        assert!(!screen.contains("Recent"), "{screen}");
        assert!(!screen.contains("cargo test"), "{screen}");
        assert!(!screen.contains("│ review"), "{screen}");
    }

    #[test]
    fn tiny_confirm_send_overlay_keeps_the_decision_visible() {
        let app = app_from_view_model_fixture(&view_fixture("confirm_dispatch_header_and_footer"));
        let lines = render_grid(&app, 60, 12);
        let screen = screen_text(&lines);

        assert_render_invariants(&lines, 60, 12);
        assert!(screen.contains("Send"), "{screen}");
        assert!(screen.contains("Review send"), "{screen}");
        assert!(screen.contains("Targets"), "{screen}");
        assert!(screen.contains("To: fleet triage"), "{screen}");
        assert!(screen.contains("Text: continue"), "{screen}");
        assert!(screen.contains("Enter send"), "{screen}");
        assert!(screen.contains("Esc cancel"), "{screen}");
    }

    #[test]
    fn short_send_overlay_keeps_lane_preview_and_overflow_summary_visible() {
        let mut first = sample_pane("codex");
        first.id = String::from("%1");

        let mut second = sample_pane("codex");
        second.id = String::from("%2");
        second.active = false;
        second.pane_index = 1;

        let mut third = sample_pane("codex");
        third.id = String::from("%3");
        third.active = false;
        third.pane_index = 2;

        let mut fourth = sample_pane("codex");
        fourth.id = String::from("%4");
        fourth.active = false;
        fourth.pane_index = 3;

        let mut fifth = sample_pane("claude");
        fifth.id = String::from("%5");
        fifth.active = false;
        fifth.window_name = String::from("review");

        let mut app = app_with_panes(vec![first, second, third, fourth, fifth], vec![]);
        app.toggle_fanout_mode();
        remember_command_for_test(&mut app, "cargo test");
        app.begin_command_input();
        for ch in "echo {id}".chars() {
            app.push_command_char(ch);
        }

        let (_, raw_lines) = app.overlay_panel().expect("send overlay should be visible");
        let body = body_rect(80, 14);
        let overlay = overlay_rect(body, "Send", &raw_lines);
        let expected = prepare_overlay_lines(
            "Send",
            raw_lines.clone(),
            overlay.width.saturating_sub(4),
            overlay.height,
        );
        let lines = render_grid(&app, 80, 14);
        let screen = screen_text(&lines);

        assert_render_invariants(&lines, 80, 14);
        assert_eq!(
            expected,
            vec![
                String::from("To: Codex lane (4 panes)"),
                String::from("Text: echo {id}"),
                String::new(),
                String::from("Preview"),
                String::from("  demo / agents #0 : echo %1"),
                String::from("  demo / agents #1 : echo %2"),
                String::from("  demo / agents #2 : echo %3"),
                String::from("  ... : 1 more pane"),
            ]
        );
        for line in &expected {
            if !line.is_empty() {
                assert!(screen.contains(line), "{screen}");
            }
        }
        assert!(
            line_index(&lines, "To: Codex lane (4 panes)") < line_index(&lines, "Preview"),
            "{screen}"
        );
        assert!(
            line_index(&lines, "  demo / agents #1 : echo %2")
                < line_index(&lines, "  ... : 1 more pane"),
            "{screen}"
        );
        assert!(!screen.contains("cargo test"), "{screen}");
        assert!(!screen.contains("vars {session}"), "{screen}");
    }

    #[test]
    fn narrow_search_mode_keeps_header_and_footer_to_single_action_rows() {
        let pane = sample_pane("codex");
        let mut app = app_with_panes(vec![pane], vec![]);
        app.begin_search();
        for ch in "approval".chars() {
            app.push_search_char(ch);
        }

        let lines = render_grid(&app, 60, 16);
        let screen = screen_text(&lines);
        let expected_header = app.header_context_line_for_width(48);
        let expected_footer = app.footer_line_for_width(60);

        assert_render_invariants(&lines, 60, 16);
        assert!(lines[0].contains(&expected_header), "{screen}");
        assert_eq!(line_indices(&lines, &expected_header).len(), 1, "{screen}");
        assert_eq!(lines[15].trim_end(), expected_footer, "{screen}");
        assert!(lines[15].contains("Enter apply"), "{screen}");
    }

    #[test]
    fn narrow_command_mode_uses_compact_footer_hint_without_wrapping() {
        let app = app_from_view_model_fixture(&view_fixture("command_input_context"));
        let lines = render_grid(&app, 60, 18);
        let screen = screen_text(&lines);
        let expected_header = app.header_context_line_for_width(48);
        let expected_footer = app.footer_line_for_width(60);

        assert_render_invariants(&lines, 60, 18);
        assert!(lines[0].contains(&expected_header), "{screen}");
        assert_eq!(line_indices(&lines, &expected_header).len(), 1, "{screen}");
        assert_eq!(lines[17].trim_end(), expected_footer, "{screen}");
        assert!(lines[17].contains("Enter send"), "{screen}");
    }

    #[test]
    fn narrow_confirm_send_keeps_compact_header_and_footer_on_single_rows() {
        let app = app_from_view_model_fixture(&view_fixture("confirm_dispatch_compact_header"));
        let lines = render_grid(&app, 60, 18);
        let screen = screen_text(&lines);
        let expected_header = app.header_context_line_for_width(48);
        let expected_footer = app.footer_line_for_width(60);

        assert_render_invariants(&lines, 60, 18);
        assert!(lines[0].contains(&expected_header), "{screen}");
        assert_eq!(line_indices(&lines, &expected_header).len(), 1, "{screen}");
        assert_eq!(lines[17].trim_end(), expected_footer, "{screen}");
        assert!(lines[17].contains("Enter send"), "{screen}");
    }

    #[test]
    fn exact_grid_matches_wide_selected_waiting_panel() {
        let fixture = panel_fixture("selected_waiting_panel");
        let app = app_from_panel_fixture(&fixture);
        let lines = render_grid(&app, 120, 18);
        assert_golden_grid(&lines, 120, "wide_selected_waiting_panel");
    }

    #[test]
    fn generic_launcher_rows_use_contextual_identity_labels_in_scannable_board_views() {
        let mut first = sample_pane("node");
        first.id = String::from("%1");
        first.window_name = String::from("node");
        first.current_path = String::from("/workspace/muxboard");

        let mut second = sample_pane("node");
        second.id = String::from("%2");
        second.window_name = String::from("node");
        second.current_path = String::from("/workspace/dotfiles");
        second.active = false;
        second.pane_index = 1;

        let app = app_with_panes(
            vec![first, second],
            vec![
                ("%1", vec!["building release artifacts"]),
                ("%2", vec!["syncing shell aliases"]),
            ],
        );

        for &(width, height) in &[(84, 16), (68, 14)] {
            let lines = render_grid(&app, width, height);
            let screen = screen_text(&lines);

            assert_render_invariants(&lines, width, height);
            assert!(screen.contains("muxboard"), "{width}x{height}\n{screen}");
            assert!(screen.contains("dotfiles"), "{width}x{height}\n{screen}");
            assert!(
                screen.contains("build release artifacts"),
                "{width}x{height}\n{screen}"
            );
            assert!(
                screen.contains("sync shell aliases"),
                "{width}x{height}\n{screen}"
            );
        }
    }

    #[test]
    fn exact_grid_matches_narrow_selected_waiting_panel() {
        let fixture = panel_fixture("selected_waiting_panel");
        let app = app_from_panel_fixture(&fixture);
        let lines = render_grid(&app, 70, 16);
        assert_golden_grid(&lines, 70, "narrow_selected_waiting_panel");
    }

    #[test]
    fn exact_grid_matches_output_overlay() {
        let fixture = panel_fixture("live_tail_with_summary_and_raw_tail");
        let app = app_from_panel_fixture(&fixture);
        let lines = render_grid(&app, 110, 18);
        assert_golden_grid(&lines, 110, "output_overlay");
    }

    #[test]
    fn exact_grid_matches_opened_output_overlay() {
        let pane = sample_pane("node");
        let mut app = app_with_panes(
            vec![pane],
            vec![(
                "%1",
                vec![
                    "Preparing workspace",
                    "Running cargo test",
                    "Checking renderer grid",
                    "Reviewing output viewport",
                    "Updating scroll model",
                    "Running ux guardrails",
                    "Capturing live tmux output",
                    "Inspecting focus geometry",
                    "Verifying panel borders",
                    "Checking footer contract",
                    "Writing release notes",
                    "Waiting on final check",
                    "Running cargo fmt",
                    "Running just ux",
                    "Running live actions",
                    "Running perf live",
                    "Running just ci",
                    "Reviewing final screen",
                    "Running final check",
                ],
            )],
        );
        let runtime = tokio::runtime::Runtime::new().expect("runtime should build");
        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Enter))
            .expect("Enter should open Output");

        let lines = render_grid(&app, 110, 18);
        assert_golden_grid(&lines, 110, "opened_output_overlay");
    }

    #[test]
    fn exact_grid_matches_actions_overlay() {
        let fixture = panel_fixture("actions_menu_sections");
        let app = app_from_panel_fixture(&fixture);
        let lines = render_grid(&app, 110, 20);
        assert_golden_grid(&lines, 110, "actions_overlay");
    }

    #[test]
    fn exact_grid_matches_fleet_picker_overlay() {
        let mut first = sample_pane("codex");
        first.id = String::from("%1");
        first.window_name = String::from("alpha");

        let mut second = sample_pane("claude");
        second.id = String::from("%2");
        second.window_id = String::from("@2");
        second.window_name = String::from("beta");
        second.active = false;

        let mut app = app_with_panes(vec![first, second], vec![]);
        app.set_target_groups_for_test(vec![
            crate::app::TargetGroup {
                name: String::from("triage"),
                members: vec![
                    crate::app::PaneLocator {
                        session_name: String::from("demo"),
                        window_name: String::from("alpha"),
                        pane_index: 0,
                    },
                    crate::app::PaneLocator {
                        session_name: String::from("demo"),
                        window_name: String::from("beta"),
                        pane_index: 0,
                    },
                ],
            },
            crate::app::TargetGroup {
                name: String::from("review"),
                members: vec![crate::app::PaneLocator {
                    session_name: String::from("demo"),
                    window_name: String::from("missing"),
                    pane_index: 0,
                }],
            },
        ]);
        app.open_fleet_picker();
        let lines = render_grid(&app, 96, 18);
        let screen = screen_text(&lines);

        assert_golden_grid(&lines, 96, "fleet_picker_overlay");
        assert_render_invariants(&lines, 96, 18);
        assert!(screen.contains("Choose a saved fleet."), "{screen}");
        assert!(screen.contains("> triage  2/2 live"), "{screen}");
        assert!(screen.contains("review  0/1 live"), "{screen}");
        assert!(
            screen.contains("J/K choose  Enter load  D delete  Esc close"),
            "{screen}"
        );
    }

    #[test]
    fn tiny_fleet_picker_overlay_keeps_selection_and_decision_visible() {
        let mut first = sample_pane("codex");
        first.id = String::from("%1");
        first.window_name = String::from("alpha");

        let mut second = sample_pane("claude");
        second.id = String::from("%2");
        second.window_id = String::from("@2");
        second.window_name = String::from("beta");

        let mut app = app_with_panes(vec![first, second], vec![]);
        app.set_target_groups_for_test(vec![
            crate::app::TargetGroup {
                name: String::from("triage"),
                members: vec![
                    crate::app::PaneLocator {
                        session_name: String::from("demo"),
                        window_name: String::from("alpha"),
                        pane_index: 0,
                    },
                    crate::app::PaneLocator {
                        session_name: String::from("demo"),
                        window_name: String::from("beta"),
                        pane_index: 0,
                    },
                ],
            },
            crate::app::TargetGroup {
                name: String::from("review"),
                members: vec![crate::app::PaneLocator {
                    session_name: String::from("demo"),
                    window_name: String::from("missing"),
                    pane_index: 0,
                }],
            },
        ]);
        app.open_fleet_picker();

        let lines = render_grid(&app, 60, 12);
        let screen = screen_text(&lines);

        assert_render_invariants(&lines, 60, 12);
        assert!(screen.contains("Fleets"), "{screen}");
        assert!(screen.contains("> triage"), "{screen}");
        assert!(screen.contains("2/2 live"), "{screen}");
        assert!(screen.contains("Enter load"), "{screen}");
        assert!(screen.contains("Esc close"), "{screen}");
    }

    #[test]
    fn tiny_fleet_picker_stale_selection_prioritizes_delete_over_load() {
        let mut pane = sample_pane("codex");
        pane.id = String::from("%1");
        pane.window_name = String::from("alpha");

        let mut app = app_with_panes(vec![pane], vec![]);
        app.set_target_groups_for_test(vec![crate::app::TargetGroup {
            name: String::from("stale"),
            members: vec![crate::app::PaneLocator {
                session_name: String::from("demo"),
                window_name: String::from("missing"),
                pane_index: 0,
            }],
        }]);
        app.open_fleet_picker();

        let lines = render_grid(&app, 60, 12);
        let screen = screen_text(&lines);
        let footer = lines.last().expect("footer should render");

        assert_render_invariants(&lines, 60, 12);
        assert!(screen.contains("Fleets"), "{screen}");
        assert!(screen.contains("> stale"), "{screen}");
        assert!(screen.contains("0/1 live"), "{screen}");
        assert!(footer.contains("D delete stale"), "{screen}");
        assert!(footer.contains("Esc close"), "{screen}");
        assert!(
            !footer.contains("Enter load"),
            "stale fleet should not present a dead load as the primary action:\n{screen}"
        );
    }

    #[test]
    fn usability_stale_active_fleet_keeps_recovery_visible_and_never_promises_send() {
        let mut pane = sample_pane("codex");
        pane.id = String::from("%1");
        pane.window_name = String::from("alpha");

        let mut app = app_with_panes(vec![pane], vec![]);
        app.set_target_groups_for_test(vec![crate::app::TargetGroup {
            name: String::from("triage"),
            members: vec![crate::app::PaneLocator {
                session_name: String::from("demo"),
                window_name: String::from("missing"),
                pane_index: 0,
            }],
        }]);
        app.load_next_target_group();

        let lines = render_grid(&app, 96, 20);
        let screen = screen_text(&lines);
        let footer = lines.last().expect("footer should render");

        assert_render_invariants(&lines, 96, 20);
        assert!(
            screen.contains("fleet triage has no live panes"),
            "{screen}"
        );
        assert!(
            screen.contains("Target: fleet triage has no live panes"),
            "{screen}"
        );
        assert!(footer.contains("fleet stale"), "{screen}");
        assert!(footer.contains(". more"), "{screen}");
        assert!(
            !footer.contains(": send"),
            "stale active fleet footer must not promise a dead send:\n{screen}"
        );

        app.open_action_menu();
        let more_lines = render_grid(&app, 96, 20);
        let more = screen_text(&more_lines);

        assert_render_invariants(&more_lines, 96, 20);
        assert!(more.contains("Action: L choose fleet"), "{more}");
        assert!(more.contains("fleet triage has no live panes"), "{more}");
        assert!(more.contains("L choose fleet"), "{more}");
        assert!(more.contains("D delete stale triage"), "{more}");
        assert!(
            !more.contains(": send text"),
            "stale active fleet More menu must not promise send text:\n{more}"
        );
    }

    #[test]
    fn usability_stale_active_fleet_footer_keeps_recovery_when_narrowed_and_details_focused() {
        let mut pane = sample_pane("codex");
        pane.id = String::from("%1");
        pane.window_name = String::from("alpha");

        let mut app = app_with_panes(vec![pane], vec![("%1", vec!["working"])]);
        app.set_target_groups_for_test(vec![crate::app::TargetGroup {
            name: String::from("triage"),
            members: vec![crate::app::PaneLocator {
                session_name: String::from("demo"),
                window_name: String::from("missing"),
                pane_index: 0,
            }],
        }]);
        app.load_next_target_group();
        app.set_search_query_for_test("alpha");
        app.cycle_context_pane();
        app.cycle_panel_focus();

        let lines = render_grid(&app, 96, 18);
        let screen = screen_text(&lines);
        let footer = lines.last().expect("footer should render");

        assert_render_invariants(&lines, 96, 18);
        assert!(screen.contains("Output"), "{screen}");
        for term in [
            "fleet stale",
            "backspace show all",
            "Esc back",
            ". more",
            "? help",
        ] {
            assert!(
                footer.contains(term),
                "stale narrowed footer lost recovery `{term}`:\n{screen}"
            );
        }
        for inert in [": send", "Enter output", "focused"] {
            assert!(
                !footer.contains(inert),
                "stale narrowed footer advertised inert/noisy `{inert}`:\n{screen}"
            );
        }
    }

    #[test]
    fn exact_grid_matches_help_overlay() {
        let mut app = app_from_view_model_fixture(&view_fixture("attention_board_row_summary"));
        app.toggle_help_overlay();
        let lines = render_grid(&app, 96, 18);
        let screen = screen_text(&lines);

        assert_golden_grid(&lines, 96, "help_overlay");
        assert_render_invariants(&lines, 96, 18);
        assert!(screen.contains("Help"), "{screen}");
        assert!(
            screen.contains("Now: : reply, Enter output, G show in tmux."),
            "{screen}"
        );
        assert!(
            screen.contains("Send: Space add/remove pane for a send list."),
            "{screen}"
        );
        assert!(
            screen.contains("Move: J/K select panes, Tab Fleet/Details."),
            "{screen}"
        );
        assert!(
            screen.contains("Legend: > selected, * active, + listed"),
            "{screen}"
        );
        assert!(screen.contains("Esc close  Q quit"), "{screen}");
    }

    #[test]
    fn exact_grid_matches_confirm_send_overlay() {
        let app = app_from_view_model_fixture(&view_fixture("confirm_dispatch_header_and_footer"));
        let lines = render_grid(&app, 100, 18);
        let screen = screen_text(&lines);

        assert_golden_grid(&lines, 100, "confirm_send_overlay");
        assert_render_invariants(&lines, 100, 18);
        assert!(
            screen.contains("Review send to fleet triage (2 panes)."),
            "{screen}"
        );
        assert!(screen.contains("Send"), "{screen}");
        assert!(!screen.contains("send to demo / agents #0"), "{screen}");
        assert!(screen.contains("Targets"), "{screen}");
        assert!(screen.contains("To: fleet triage (2 panes)"), "{screen}");
        assert!(screen.contains("Text: continue"), "{screen}");
        assert!(
            screen.contains("? help  Enter send  Esc cancel"),
            "{screen}"
        );
    }

    #[test]
    fn exact_grid_matches_overview_attention_overlay() {
        let fixture = panel_fixture("overview_panel_with_attention");
        let app = app_from_panel_fixture(&fixture);
        let lines = render_grid(&app, 110, 18);
        assert_golden_grid(&lines, 110, "overview_attention_overlay");
    }

    #[test]
    fn exact_grid_matches_multi_attention_command_center() {
        let mut panes = Vec::new();
        let mut pane_ids = Vec::new();

        for index in 0..8 {
            let mut pane = sample_pane("codex");
            pane.id = format!("%{}", index + 1);
            pane.window_id = format!("@{index}");
            pane.window_name = format!("agent-{index}");
            pane.active = index == 0;
            pane.pane_index = index;
            pane_ids.push(pane.id.clone());
            panes.push(pane);
        }

        let runtimes = pane_ids
            .iter()
            .map(|id| (id.as_str(), vec!["Press Enter to continue."]))
            .collect::<Vec<_>>();
        let mut app = app_with_panes(panes, runtimes);
        app.show_command_center();

        let lines = render_grid(&app, 120, 22);
        assert_golden_grid(&lines, 120, "multi_attention_command_center");
    }

    fn mixed_loop_app() -> App {
        let mut waiting = sample_pane("claude");
        waiting.id = String::from("%1");
        waiting.window_id = String::from("@1");
        waiting.window_name = String::from("approval");

        let mut running = sample_pane("codex");
        running.id = String::from("%2");
        running.window_id = String::from("@2");
        running.window_name = String::from("build");
        running.active = false;
        running.pane_index = 1;

        let mut quiet = sample_pane("zsh");
        quiet.id = String::from("%3");
        quiet.window_id = String::from("@3");
        quiet.window_name = String::from("shell");
        quiet.active = false;
        quiet.pane_index = 2;

        let mut checking = sample_pane("node");
        checking.id = String::from("%4");
        checking.window_id = String::from("@4");
        checking.window_name = String::from("pending");
        checking.active = false;
        checking.pane_index = 3;

        app_with_panes(
            vec![waiting, running, quiet, checking],
            vec![
                ("%1", vec!["Waiting for leader to approve network access."]),
                ("%2", vec!["Running cargo test"]),
                ("%3", vec!["$"]),
            ],
        )
    }

    #[test]
    fn exact_grid_matches_mixed_fleet_dashboard() {
        let app = mixed_loop_app();
        let lines = render_grid(&app, 120, 18);
        assert_golden_grid(&lines, 120, "mixed_fleet_dashboard");
    }

    #[test]
    fn exact_grid_matches_mixed_fleet_after_moving_to_checking() {
        let mut app = mixed_loop_app();
        let runtime = tokio::runtime::Runtime::new().expect("runtime should build");
        for _ in 0..3 {
            runtime
                .block_on(handle_key_press(&mut app, KeyCode::Char('j')))
                .expect("J should move through the mixed fleet");
        }

        let lines = render_grid(&app, 120, 18);
        let screen = screen_text(&lines);
        let footer = lines.last().expect("footer should render");

        assert_golden_grid(&lines, 120, "mixed_fleet_after_moving_to_checking");
        assert!(screen.contains(">+ demo/pending  checking"), "{screen}");
        assert!(screen.contains("State: Checking"), "{screen}");
        assert!(screen.contains("Action: G show in tmux"), "{screen}");
        assert!(footer.contains("G show"), "{screen}");
        assert!(
            !footer.contains("Enter output"),
            "checking panes without useful output should not advertise an empty Output detour:\n{screen}"
        );
        assert!(!screen.contains("unknown"), "{screen}");
        assert!(
            !footer.contains(": reply"),
            "checking panes should not keep the previous waiting reply hint:\n{screen}"
        );
    }

    #[test]
    fn exact_grid_matches_command_center_from_working_selection() {
        let mut app = mixed_loop_app();
        app.select_next_pane();
        app.show_command_center();

        let lines = render_grid(&app, 120, 20);
        let screen = screen_text(&lines);
        let footer = lines.last().expect("footer should render");

        assert_golden_grid(&lines, 120, "command_center_from_working_selection");
        assert!(screen.contains("Action: : reply to demo / approval"));
        assert!(screen.contains("Selected: demo / build"));
        assert!(footer.contains(": reply"), "{screen}");
        assert!(
            footer.find(": reply") < footer.find("G show"),
            "footer should lead with reply before the intentional attach path:\n{screen}"
        );
    }

    #[test]
    fn exact_grid_matches_empty_command_center() {
        let mut app = app_with_panes(Vec::new(), vec![]);
        app.show_command_center();

        let lines = render_grid(&app, 100, 16);
        assert_golden_grid(&lines, 100, "empty_command_center");
    }

    #[test]
    fn exact_grid_matches_no_match_command_center() {
        let mut app = app_with_panes(
            vec![sample_pane("codex")],
            vec![("%1", vec!["Press Enter to continue."])],
        );
        app.show_command_center();
        app.begin_search();
        for ch in "zzz-no-match".chars() {
            app.push_search_char(ch);
        }
        app.finish_search();

        let lines = render_grid(&app, 100, 16);
        assert_golden_grid(&lines, 100, "no_match_command_center");
    }

    #[test]
    fn exact_grid_matches_working_agent_dashboard() {
        let mut running = sample_pane("codex");
        running.id = String::from("%1");
        running.window_id = String::from("@1");
        running.window_name = String::from("build");

        let app = app_with_panes(
            vec![running],
            vec![(
                "%1",
                vec![
                    "Running cargo test --all-targets --all-features",
                    "Compiling muxboard v1.0.0",
                    "Running renderer golden tests",
                    "Checking command center queue overflow",
                    "Running live tmux action contract",
                    "Reviewing final screen for confusing copy",
                ],
            )],
        );

        let lines = render_grid(&app, 120, 18);
        assert_golden_grid(&lines, 120, "working_agent_dashboard");
    }

    #[test]
    fn exact_grid_matches_empty_navigator_overlay() {
        let fixture = panel_fixture("navigator_empty_state");
        let app = app_from_panel_fixture(&fixture);
        let lines = render_grid(&app, 100, 16);
        assert_golden_grid(&lines, 100, "empty_navigator_overlay");
    }

    #[test]
    fn exact_grid_matches_empty_output_overlay() {
        let fixture = panel_fixture("live_tail_empty_state");
        let app = app_from_panel_fixture(&fixture);
        let lines = render_grid(&app, 100, 16);
        assert_golden_grid(&lines, 100, "empty_output_overlay");
    }

    #[test]
    fn exact_grid_matches_command_input_mode() {
        let app = app_from_view_model_fixture(&view_fixture("command_input_context"));
        let lines = render_grid(&app, 100, 18);
        assert_golden_grid(&lines, 100, "command_input_mode");
    }

    #[test]
    fn exact_grid_matches_start_agent_overlay() {
        let mut pane = sample_pane("node");
        pane.current_path = String::from("/workspace/muxboard");
        let mut app = app_with_panes(vec![pane], vec![("%1", vec!["codex is working"])]);
        app.begin_launch_input();

        let lines = render_grid(&app, 100, 18);
        let screen = screen_text(&lines);

        assert_golden_grid(&lines, 100, "start_agent_overlay");
        assert_render_invariants(&lines, 100, 18);
        assert!(screen.contains("Start agent."), "{screen}");
        assert!(screen.contains("In: demo / agents"), "{screen}");
        assert!(screen.contains("Folder: /workspace/muxboard"), "{screen}");
        assert!(screen.contains("Command: codex"), "{screen}");
        assert!(screen.contains("codex"), "{screen}");
        assert!(screen.contains("Tab preset"), "{screen}");
        assert!(screen.contains("Enter start"), "{screen}");
    }

    #[test]
    fn narrow_start_agent_overlay_keeps_destination_command_and_recovery() {
        let mut pane = sample_pane("node");
        pane.current_path = String::from("/workspace/a-very-long-project-path/muxboard");
        let mut app = app_with_panes(vec![pane], vec![("%1", vec!["codex is working"])]);
        app.begin_launch_input();

        let lines = render_grid(&app, 68, 14);
        let screen = screen_text(&lines);

        assert_render_invariants(&lines, 68, 14);
        assert!(screen.contains("Start"), "{screen}");
        assert!(screen.contains("demo / agents"), "{screen}");
        assert!(screen.contains("Folder:"), "{screen}");
        assert!(screen.contains("Command:"), "{screen}");
        assert!(screen.contains("codex"), "{screen}");
        assert!(!screen.contains("? help"), "{screen}");
        assert!(screen.contains("Esc cancel"), "{screen}");
        assert!(screen.contains("Enter start"), "{screen}");
        assert!(!screen.contains("launch agent pane"), "{screen}");
    }

    #[test]
    fn tiny_start_agent_overlay_keeps_destination_command_and_recovery() {
        let mut pane = sample_pane("node");
        pane.current_path = String::from("/workspace/muxboard");
        let mut app = app_with_panes(vec![pane], vec![("%1", vec!["codex is working"])]);
        app.begin_launch_input();

        let lines = render_grid(&app, 60, 12);
        let screen = screen_text(&lines);

        assert_render_invariants(&lines, 60, 12);
        assert!(screen.contains("Start"), "{screen}");
        assert!(screen.contains("demo / agents"), "{screen}");
        assert!(screen.contains("Command:"), "{screen}");
        assert!(screen.contains("codex"), "{screen}");
        assert!(screen.contains("Enter start"), "{screen}");
        assert!(screen.contains("Esc cancel"), "{screen}");
    }

    #[test]
    fn start_agent_error_overlay_keeps_command_and_recovery_visible() {
        let mut pane = sample_pane("bash");
        pane.current_path = String::from("/workspace/muxboard");
        let mut app = app_with_panes(vec![pane], vec![]);
        app.begin_launch_input();
        for ch in "codex".chars() {
            app.push_launch_char(ch);
        }
        app.set_status_message_for_test("Start failed: new-window refused");

        let lines = render_grid(&app, 90, 16);
        let screen = screen_text(&lines);

        assert_render_invariants(&lines, 90, 16);
        assert!(screen.contains("Error:"), "{screen}");
        assert!(screen.contains("codex"), "{screen}");
        assert!(screen.contains("Enter start"), "{screen}");
        assert!(screen.contains("Esc cancel"), "{screen}");
    }

    #[test]
    fn start_agent_missing_target_overlay_hides_inert_start_action() {
        let pane = sample_pane("bash");
        let mut app = app_with_panes(vec![pane], vec![]);
        app.begin_launch_input();
        app.set_selected_pane_id_for_test(None);
        app.set_status_message_for_test("Select a pane first.");

        let lines = render_grid(&app, 80, 14);
        let screen = screen_text(&lines);

        assert_render_invariants(&lines, 80, 14);
        assert!(screen.contains("In: select a pane"), "{screen}");
        assert!(
            screen.contains("Action: Esc cancel, then choose a pane"),
            "{screen}"
        );
        assert!(screen.contains("Command:"), "{screen}");
        assert!(
            screen.contains("Esc cancel, then choose a pane"),
            "{screen}"
        );
        assert!(!screen.contains("Enter start"), "{screen}");
        assert!(!screen.contains("Folder:"), "{screen}");

        let target_y = line_index(&lines, "In: select a pane");
        let action_y = line_index(&lines, "Action: Esc cancel, then choose a pane");
        let command_y = line_index(&lines, "Command:");
        assert_eq!(action_y, target_y + 1, "{screen}");
        assert!(command_y > action_y, "{screen}");
    }

    #[test]
    fn exact_grid_matches_empty_search_board() {
        let app = app_from_view_model_fixture(&view_fixture("empty_search_result_board_title"));
        let lines = render_grid(&app, 80, 16);
        assert_golden_grid(&lines, 80, "empty_search_board");
    }

    #[test]
    fn exact_grid_matches_empty_tmux_board() {
        let app = app_with_panes(Vec::new(), vec![]);
        let lines = render_grid(&app, 80, 16);
        assert_golden_grid(&lines, 80, "empty_tmux_board");
    }

    #[test]
    fn responsive_selected_layout_holds_across_width_and_height_ranges() {
        let fixture = panel_fixture("selected_waiting_panel");
        let app = app_from_panel_fixture(&fixture);

        for &(width, height) in &[(68, 14), (70, 16), (84, 16), (96, 18), (120, 18), (140, 24)] {
            let lines = render_grid(&app, width, height);
            let screen = screen_text(&lines);
            assert_render_invariants(&lines, width, height);
            assert!(screen.contains("muxboard"), "{width}x{height}\n{screen}");
            assert!(screen.contains("Fleet"), "{width}x{height}\n{screen}");
            assert!(screen.contains("Details"), "{width}x{height}\n{screen}");
            assert!(screen.contains("Waiting"), "{width}x{height}\n{screen}");
            assert!(screen.contains("? help"), "{width}x{height}\n{screen}");
        }
    }

    #[test]
    fn responsive_overlay_layouts_hold_across_sizes() {
        let mut help = app_from_view_model_fixture(&view_fixture("attention_board_row_summary"));
        help.toggle_help_overlay();
        let actions = app_from_panel_fixture(&panel_fixture("actions_menu_sections"));
        let output = app_from_panel_fixture(&panel_fixture("live_tail_with_summary_and_raw_tail"));
        let browse = app_from_panel_fixture(&panel_fixture("navigator_empty_state"));

        for &(title, app) in &[
            ("Help", &help),
            ("More", &actions),
            ("Output", &output),
            ("Browse", &browse),
        ] {
            for &(width, height) in &[(80, 16), (96, 18), (110, 20), (140, 24)] {
                let lines = render_grid(app, width, height);
                let screen = screen_text(&lines);
                assert_render_invariants(&lines, width, height);
                assert!(screen.contains(title), "{title} {width}x{height}\n{screen}");
                if title == "Help" {
                    assert!(
                        screen.contains("Esc close"),
                        "{title} {width}x{height}\n{screen}"
                    );
                    assert!(
                        !screen.contains("? help"),
                        "{title} should not advertise opening Help while Help is open:\n{screen}"
                    );
                } else {
                    assert!(
                        screen.contains("? help"),
                        "{title} {width}x{height}\n{screen}"
                    );
                }
            }
        }
    }

    #[test]
    fn chrome_trunk_test_identifies_location_and_primary_action_across_modes() {
        let mut help = app_from_view_model_fixture(&view_fixture("attention_board_row_summary"));
        help.toggle_help_overlay();

        let scenarios = vec![
            (
                "selected",
                app_from_panel_fixture(&panel_fixture("selected_waiting_panel")),
                "demo/agents",
                "Details",
                "Enter output",
            ),
            (
                "output",
                app_from_panel_fixture(&panel_fixture("live_tail_with_summary_and_raw_tail")),
                "Output",
                "Output",
                "Esc back",
            ),
            (
                "overview",
                app_from_panel_fixture(&panel_fixture("overview_panel_with_attention")),
                "Command Center",
                "Command Center",
                "G show",
            ),
            (
                "browse",
                app_from_panel_fixture(&panel_fixture("navigator_empty_state")),
                "Browse",
                "Browse",
                "backspace show all",
            ),
            (
                "more",
                app_from_panel_fixture(&panel_fixture("actions_menu_sections")),
                "More",
                "More",
                "Esc close",
            ),
            (
                "send",
                app_from_view_model_fixture(&view_fixture("command_input_context")),
                "Send to",
                "Send",
                "Enter send",
            ),
            (
                "review",
                app_from_view_model_fixture(&view_fixture("confirm_dispatch_header_and_footer")),
                "Review send",
                "Send",
                "Enter send",
            ),
            ("help", help, "Help", "Help", "Esc close"),
        ];

        for (name, app, header_term, body_term, footer_term) in scenarios {
            let lines = render_grid(&app, 100, 18);
            let screen = screen_text(&lines);
            assert_render_invariants(&lines, 100, 18);
            assert!(lines[0].contains("muxboard"), "{name}\n{screen}");
            assert!(
                lines[0].contains(header_term),
                "{name} header should identify location `{header_term}`\n{screen}"
            );
            assert!(
                screen.contains(body_term),
                "{name} body should identify major section `{body_term}`\n{screen}"
            );
            let text_entry_footer = lines[17].contains("type ") && lines[17].contains("Esc cancel");
            let help_overlay_footer = name == "help" && lines[17].starts_with("Esc close");
            if help_overlay_footer {
                assert!(
                    !screen.contains("? help"),
                    "{name} footer should not advertise opening Help while Help is open\n{screen}"
                );
            } else if text_entry_footer {
                assert!(
                    !lines[17].contains("? help"),
                    "{name} footer should not advertise ? help while ? is valid text\n{screen}"
                );
            } else {
                assert!(
                    lines[17].contains("? help"),
                    "{name} footer should keep help discoverable\n{screen}"
                );
            }
            assert!(
                lines[17].contains(footer_term),
                "{name} footer should expose `{footer_term}`\n{screen}"
            );
        }
    }

    #[test]
    fn usability_action_hierarchy_keeps_primary_actions_visible_across_surfaces() {
        fn assert_surface(
            name: &str,
            app: &App,
            width: u16,
            height: u16,
            must_show: &[&str],
            must_hide: &[&str],
        ) -> Vec<String> {
            let lines = render_grid(app, width, height);
            let screen = screen_text(&lines);
            assert_render_invariants(&lines, width, height);
            for term in must_show {
                assert!(
                    screen.contains(term),
                    "{name} {width}x{height} missing `{term}`:\n{screen}"
                );
            }
            for term in must_hide {
                assert!(
                    !screen.contains(term),
                    "{name} {width}x{height} advertised inert `{term}`:\n{screen}"
                );
            }
            lines
        }

        let main = app_from_panel_fixture(&panel_fixture("selected_waiting_panel"));
        for &(width, height) in &[(80, 16), (120, 18), (140, 24)] {
            let main_lines = assert_surface(
                "main",
                &main,
                width,
                height,
                &[
                    "Fleet",
                    "Details",
                    "? help",
                    "J/K move",
                    ": reply",
                    "Enter output",
                ],
                &[],
            );
            let footer = main_lines.last().expect("main should render footer");
            assert!(
                footer.find(": reply") < footer.find("Enter output"),
                "main footer should put the primary reply before the secondary output peek:\n{}",
                screen_text(&main_lines)
            );
        }

        let mut help = app_from_view_model_fixture(&view_fixture("attention_board_row_summary"));
        help.toggle_help_overlay();
        for &(width, height) in &[(80, 16), (120, 20)] {
            assert_surface(
                "help",
                &help,
                width,
                height,
                &["Help", "Now:", "More:", "Esc close"],
                &["? help"],
            );
        }

        let more = app_from_panel_fixture(&panel_fixture("actions_menu_sections"));
        assert_surface(
            "tiny More",
            &more,
            60,
            12,
            &["More", "+ start agent", "] command center"],
            &[],
        );
        let normal_more = assert_surface(
            "normal More",
            &more,
            80,
            24,
            &["More", "S summarize panes", "+ start agent", "Settings"],
            &[],
        );
        assert!(
            !screen_text(&normal_more).contains("L layout: auto")
                || line_index(&normal_more, "S summarize panes")
                    < line_index(&normal_more, "L layout: auto"),
            "normal More must keep summary above lower-value layout:\n{}",
            screen_text(&normal_more)
        );
        let wide_more = assert_surface(
            "wide More",
            &more,
            130,
            36,
            &["More", "S summarize panes", "L layout: auto", "Settings"],
            &[],
        );
        assert!(
            line_index(&wide_more, "S summarize panes") < line_index(&wide_more, "L layout: auto"),
            "wide More must keep summary above lower-value layout:\n{}",
            screen_text(&wide_more)
        );

        let mut first = sample_pane("codex");
        first.id = String::from("%1");
        let mut second = sample_pane("claude");
        second.id = String::from("%2");
        second.active = false;
        second.pane_index = 1;
        let mut save_fleet = app_with_panes(vec![first, second], vec![]);
        save_fleet.toggle_selected_mark();
        save_fleet.select_next_pane();
        save_fleet.toggle_selected_mark();
        save_fleet.open_action_menu();
        for &(width, height) in &[(80, 20), (120, 24)] {
            assert_surface(
                "save-fleet More",
                &save_fleet,
                width,
                height,
                &["More", "Send List", "X clear send list", "G save fleet"],
                &[],
            );
        }

        let command_center =
            app_from_panel_fixture(&panel_fixture("overview_panel_with_attention"));
        for &(width, height) in &[(80, 16), (120, 24)] {
            assert_surface(
                "Command Center",
                &command_center,
                width,
                height,
                &["Command Center", "Needs you", "G show", "? help"],
                &[],
            );
        }

        let browse = app_from_panel_fixture(&panel_fixture("navigator_empty_state"));
        for &(width, height) in &[(80, 16), (120, 20)] {
            assert_surface(
                "Browse",
                &browse,
                width,
                height,
                &["Browse", "backspace show all", "? help", "Esc back"],
                &[": send", "Space add"],
            );
        }

        let output = app_from_panel_fixture(&panel_fixture("live_tail_with_summary_and_raw_tail"));
        for &(width, height) in &[(80, 16), (120, 18)] {
            assert_surface(
                "Output",
                &output,
                width,
                height,
                &["Output", "Summary", "Esc back", "J/K move"],
                &["Enter output"],
            );
        }
        let mut scrolling_output = app_with_panes(
            vec![sample_pane("node")],
            vec![(
                "%1",
                vec![
                    "step 01 plan",
                    "step 02 fetch",
                    "step 03 build",
                    "step 04 test",
                    "step 05 package",
                    "step 06 upload",
                    "step 07 notify",
                    "step 08 verify",
                    "step 09 snapshot",
                    "step 10 render",
                    "step 11 inspect",
                    "step 12 patch",
                    "step 13 live",
                    "step 14 perf",
                    "step 15 ci",
                    "step 16 audit",
                    "step 17 smoke",
                    "step 18 done",
                ],
            )],
        );
        tokio::runtime::Runtime::new()
            .expect("runtime should build")
            .block_on(handle_key_press(&mut scrolling_output, KeyCode::Enter))
            .expect("enter should open scrollable Output");
        for &(width, height) in &[(80, 16), (120, 18)] {
            assert_surface(
                "scrollable Output",
                &scrolling_output,
                width,
                height,
                &["Output", "Latest", "Esc back", "K older/J newer"],
                &["Enter output"],
            );
        }

        let send = app_from_view_model_fixture(&view_fixture("command_input_context"));
        for &(width, height) in &[(80, 16), (120, 20)] {
            assert_surface(
                "Send",
                &send,
                width,
                height,
                &["Send", "type text", "Enter send", "Esc cancel"],
                &["? help"],
            );
        }

        let review =
            app_from_view_model_fixture(&view_fixture("confirm_dispatch_header_and_footer"));
        for &(width, height) in &[(80, 16), (120, 20)] {
            assert_surface(
                "review send",
                &review,
                width,
                height,
                &["Review send", "Enter send", "Esc cancel"],
                &[": send text"],
            );
        }

        let empty = app_with_panes(Vec::new(), vec![]);
        for &(width, height) in &[(60, 12), (80, 16)] {
            assert_surface(
                "empty",
                &empty,
                width,
                height,
                &["No panes yet", "R refresh", "? help"],
                &["Enter output", ": send", "Space add"],
            );
        }

        let mut no_match = app_with_panes(vec![sample_pane("codex")], vec![]);
        no_match.set_search_query_for_test("zz-no-match");
        for &(width, height) in &[(80, 16), (120, 20)] {
            assert_surface(
                "narrowed no match",
                &no_match,
                width,
                height,
                &["no matches", "backspace show all", "/ filter", ". more"],
                &["Enter output", ": send", "Space add"],
            );
        }
    }

    #[test]
    fn header_and_row_styles_keep_visual_hierarchy() {
        let fixture = panel_fixture("selected_waiting_panel");
        let app = app_from_panel_fixture(&fixture);
        let theme = Theme::from_preset(app.theme_preset());
        let buffer = render_buffer(&app, 120, 18);
        let lines = buffer_grid_lines(&buffer);

        let brand = buffer_cell(&buffer, &lines, "muxboard");
        assert_eq!(brand.fg, theme.accent);
        assert!(brand.modifier.contains(Modifier::BOLD));

        let board_corner = buffer.cell((0, 1)).expect("board border should exist");
        assert_eq!(board_corner.fg, theme.accent);

        let selected_corner = buffer.cell((66, 1)).expect("selected border should exist");
        assert_eq!(selected_corner.fg, theme.muted);

        let selected_row_y = lines
            .iter()
            .position(|line| line.contains(">! demo/agents"))
            .expect("selected fleet row should render");
        let selected_row = buffer_cell_in_line(
            &buffer,
            &lines[selected_row_y],
            selected_row_y,
            "demo/agents",
        );
        assert_eq!(selected_row.fg, theme.selected_fg);
        assert_eq!(selected_row.bg, theme.selected_bg);
        assert!(selected_row.modifier.contains(Modifier::BOLD));

        let footer = buffer_cell(&buffer, &lines, "? help");
        assert_eq!(footer.fg, theme.text);
    }

    #[test]
    fn usability_panel_focus_has_a_visible_render_state() {
        let fixture = panel_fixture("selected_waiting_panel");
        let mut app = app_from_panel_fixture(&fixture);
        let theme = Theme::from_preset(app.theme_preset());

        let buffer = render_buffer(&app, 120, 18);
        let lines = buffer_grid_lines(&buffer);
        let fleet_corner = buffer.cell((0, 1)).expect("fleet border should exist");
        let details_corner = buffer.cell((66, 1)).expect("details border should exist");
        assert_eq!(fleet_corner.fg, theme.accent);
        assert_eq!(details_corner.fg, theme.muted);
        assert!(screen_text(&lines).contains("Tab focus"));

        app.cycle_panel_focus();
        let buffer = render_buffer(&app, 120, 18);
        let lines = buffer_grid_lines(&buffer);
        let fleet_corner = buffer.cell((0, 1)).expect("fleet border should exist");
        let details_corner = buffer.cell((66, 1)).expect("details border should exist");
        assert_eq!(fleet_corner.fg, theme.muted);
        assert_eq!(details_corner.fg, theme.accent);
        assert!(screen_text(&lines).contains("J/K move"));
        assert!(screen_text(&lines).contains("Enter output"));
        assert!(app.status_hint_line_for_width(120).contains("J/K move"));
    }

    #[test]
    fn usability_enter_output_moves_focus_to_the_scrollable_details_surface() {
        let pane = sample_pane("bash");
        let mut app = app_with_panes(
            vec![pane],
            vec![(
                "%1",
                vec![
                    "step 01 plan",
                    "step 02 fetch",
                    "step 03 build",
                    "step 04 test",
                    "step 05 package",
                    "step 06 upload",
                    "step 07 notify",
                    "step 08 verify",
                    "step 09 snapshot",
                    "step 10 render",
                    "step 11 inspect",
                    "step 12 patch",
                    "step 13 live",
                    "step 14 perf",
                    "step 15 ci",
                    "step 16 audit",
                    "step 17 smoke",
                    "step 18 done",
                ],
            )],
        );
        let runtime = tokio::runtime::Runtime::new().expect("runtime should build");

        let before = render_grid(&app, 120, 18);
        let before_screen = screen_text(&before);
        assert_render_invariants(&before, 120, 18);
        assert!(before.last().is_some_and(|line| line.contains("J/K move")));
        assert!(before_screen.contains("Enter output"), "{before_screen}");

        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Enter))
            .expect("enter should open output without tmux");
        let after = render_grid(&app, 120, 18);
        let after_screen = screen_text(&after);
        let footer = after.last().expect("footer should render");

        assert_render_invariants(&after, 120, 18);
        assert!(app.is_details_panel_focused());
        assert!(after_screen.contains("Output"), "{after_screen}");
        assert!(footer.contains("? help"), "{after_screen}");
        assert!(footer.contains("K older/J newer"), "{after_screen}");
        assert!(footer.contains("Esc back"), "{after_screen}");
        assert!(
            !footer.contains("Enter details"),
            "Enter should not be advertised as a backward action:\n{after_screen}"
        );
        assert!(
            !footer.contains("J/K move"),
            "output mode should make scrolling the obvious next move:\n{after_screen}"
        );

        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Char('k')))
            .expect("K should scroll to older output");
        let scrolled_lines = render_grid(&app, 120, 18);
        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Enter))
            .expect("enter should keep output open");
        assert_eq!(app.context_panel_title(), "Output");
        assert_eq!(
            render_grid(&app, 120, 18),
            scrolled_lines,
            "Enter should not reset output scroll or move backward"
        );

        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Esc))
            .expect("escape should leave Output without a scroll trap");
        assert_eq!(app.context_panel_title(), "Details");
        assert!(app.is_fleet_panel_focused());
        let details_lines = render_grid(&app, 120, 18);
        let details_screen = screen_text(&details_lines);
        assert!(details_screen.contains("Details"), "{details_screen}");
        assert!(
            details_lines
                .last()
                .is_some_and(|line| !line.contains("Esc back")),
            "{details_screen}"
        );
    }

    #[test]
    fn usability_empty_output_does_not_advertise_inert_scroll() {
        let mut first = sample_pane("bash");
        first.id = String::from("%1");
        first.window_name = String::from("alpha");

        let mut second = sample_pane("zsh");
        second.id = String::from("%2");
        second.window_name = String::from("beta");
        second.active = false;
        second.pane_index = 1;

        let mut app = app_with_panes(vec![first, second], vec![]);
        let runtime = tokio::runtime::Runtime::new().expect("runtime should build");

        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Enter))
            .expect("enter should open output without tmux");

        let lines = render_grid(&app, 100, 16);
        let screen = screen_text(&lines);
        let footer = lines.last().expect("footer should render");

        assert_render_invariants(&lines, 100, 16);
        assert!(screen.contains("Output"), "{screen}");
        assert!(screen.contains("No output yet."), "{screen}");
        assert!(!footer.contains("J/K move"), "{screen}");
        assert!(footer.contains("Esc back"), "{screen}");
        assert!(
            !footer.contains("K older/J newer"),
            "empty output must not advertise an inert scroll action:\n{screen}"
        );
    }

    #[test]
    fn usability_output_footer_scroll_keys_scroll_output_not_fleet() {
        let mut first = sample_pane("bash");
        first.id = String::from("%1");
        first.window_name = String::from("alpha");

        let mut second = sample_pane("codex");
        second.id = String::from("%2");
        second.window_id = String::from("@2");
        second.window_name = String::from("beta");
        second.active = false;
        second.pane_index = 1;

        let output = vec![
            "download crates from registry",
            "compile workspace parser module",
            "compile workspace renderer module",
            "compile workspace tmux module",
            "run unit tests for provider parsing",
            "run unit tests for command center",
            "run renderer snapshot checks",
            "run live tmux smoke harness",
            "collect navigation latency sample",
            "collect output scroll latency sample",
            "package release binary",
            "verify release archive contents",
            "write final release notes",
            "sync artifacts to staging",
            "verify staging checksum",
            "promote staging build",
            "notify waiting panes",
            "finish release workflow",
            "archive signed binaries",
            "publish release announcement",
            "sync release dashboard",
            "final archive checksum sent",
        ];
        let mut app = app_with_panes(vec![first, second], vec![("%1", output)]);
        let runtime = tokio::runtime::Runtime::new().expect("runtime should build");

        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Enter))
            .expect("enter should open output without tmux");
        let before_lines = render_grid(&app, 100, 16);
        let before = screen_text(&before_lines);
        let before_footer = before_lines.last().expect("footer should render");
        let selected_before = app
            .board_rows(100)
            .into_iter()
            .find(|row| row.selected)
            .expect("one row should be selected")
            .pane;

        assert_render_invariants(&before_lines, 100, 16);
        assert!(before.contains("Output"), "{before}");
        assert!(before_footer.contains("K older/J newer"), "{before}");

        for _ in 0..3 {
            runtime
                .block_on(handle_key_press(&mut app, KeyCode::Char('k')))
                .expect("advertised scroll key should run");
        }
        let scrolled_lines = render_grid(&app, 100, 16);
        let scrolled = screen_text(&scrolled_lines);
        let selected_after = app
            .board_rows(100)
            .into_iter()
            .find(|row| row.selected)
            .expect("one row should stay selected")
            .pane;

        assert_render_invariants(&scrolled_lines, 100, 16);
        assert_eq!(
            selected_after, selected_before,
            "K older/J newer in Output must not move Fleet selection:\n{scrolled}"
        );
        assert_ne!(
            normalize_relative_ages(scrolled_lines.clone()),
            normalize_relative_ages(before_lines.clone()),
            "K should visibly scroll to older Output without hiding the viewport:\n{scrolled}"
        );
        assert!(
            scrolled.contains("sync artifacts to staging"),
            "scrolling older should reveal the previous contiguous output window:\n{scrolled}"
        );
        assert!(
            !scrolled.contains("final archive checksum sent"),
            "scrolling older should not keep pinning the newest tail line:\n{scrolled}"
        );
        assert!(
            !scrolled.contains("scrolled"),
            "scroll feedback must not replace the footer keymap:\n{scrolled}"
        );

        for _ in 0..3 {
            runtime
                .block_on(handle_key_press(&mut app, KeyCode::Char('j')))
                .expect("advertised scroll key should run");
        }
        assert_eq!(
            normalize_relative_ages(render_grid(&app, 100, 16)),
            normalize_relative_ages(before_lines),
            "J should recover the newest Output viewport"
        );
    }

    #[test]
    fn usability_page_home_end_keys_scroll_output_without_moving_fleet() {
        let output = (1..=40)
            .map(|index| format!("page output {index:02}"))
            .collect::<Vec<_>>();
        let output_refs = output.iter().map(String::as_str).collect::<Vec<_>>();
        let mut app = app_with_panes(vec![sample_pane("bash")], vec![("%1", output_refs)]);
        let runtime = tokio::runtime::Runtime::new().expect("runtime should build");

        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Enter))
            .expect("enter should open output");
        let selected_before = app
            .board_rows(100)
            .into_iter()
            .find(|row| row.selected)
            .expect("one row should be selected")
            .pane;

        runtime
            .block_on(handle_key_press(&mut app, KeyCode::PageUp))
            .expect("page up should scroll older");
        let paged = screen_text(&render_grid(&app, 100, 18));
        assert!(paged.contains("page output 2"), "{paged}");

        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Home))
            .expect("home should jump to oldest output");
        let oldest = render_grid(&app, 100, 18);
        let oldest_screen = screen_text(&oldest);
        assert!(oldest_screen.contains("page output 01"), "{oldest_screen}");
        assert_scrollbar_thumb_at_top(&oldest);

        runtime
            .block_on(handle_key_press(&mut app, KeyCode::End))
            .expect("end should jump to newest output");
        let newest = render_grid(&app, 100, 18);
        let newest_screen = screen_text(&newest);
        assert!(newest_screen.contains("page output 40"), "{newest_screen}");
        assert_scrollbar_thumb_at_bottom(&newest);

        runtime
            .block_on(handle_key_press(&mut app, KeyCode::PageDown))
            .expect("page down should be safe at newest output");
        let selected_after = app
            .board_rows(100)
            .into_iter()
            .find(|row| row.selected)
            .expect("one row should stay selected")
            .pane;
        assert_eq!(
            selected_after, selected_before,
            "scroll paging keys must not move Fleet selection"
        );
    }

    #[test]
    fn usability_details_and_output_scroll_preserve_chrome_and_headings() {
        let output = vec![
            "download crates from registry",
            "compile workspace parser module",
            "compile workspace renderer module",
            "compile workspace tmux module",
            "run unit tests for provider parsing",
            "run unit tests for command center",
            "run renderer snapshot checks",
            "run live tmux smoke harness",
            "collect navigation latency sample",
            "collect output scroll latency sample",
            "package release binary",
            "verify release archive contents",
            "write final release notes",
            "sync artifacts to staging",
            "verify staging checksum",
            "promote staging build",
            "notify waiting panes",
            "finish release workflow",
        ];
        let mut details = app_with_panes(vec![sample_pane("bash")], vec![("%1", output.clone())]);
        details.cycle_panel_focus();
        let details_before = render_grid(&details, 120, 30);
        details.select_previous_pane();
        let details_after = render_grid(&details, 120, 30);

        assert_render_invariants(&details_before, 120, 30);
        assert_render_invariants(&details_after, 120, 30);
        for needle in ["Fleet", "Details", "Output"] {
            assert_eq!(
                text_position(&details_before, needle),
                text_position(&details_after, needle),
                "{needle} should stay anchored while Details scrolls:\nbefore:\n{}\n\nafter:\n{}",
                screen_text(&details_before),
                screen_text(&details_after)
            );
        }
        assert_eq!(
            panel_border_signature(&details_before),
            panel_border_signature(&details_after),
            "Details scrolling must not move panel chrome"
        );
        let details_scrolled = screen_text(&details_after);
        assert!(details_scrolled.contains("verify release archive contents"));
        assert!(!details_scrolled.contains("final archive checksum sent"));

        let mut output_app = app_with_panes(vec![sample_pane("bash")], vec![("%1", output)]);
        let runtime = tokio::runtime::Runtime::new().expect("runtime should build");
        runtime
            .block_on(handle_key_press(&mut output_app, KeyCode::Enter))
            .expect("enter should open output");
        let output_before = render_grid(&output_app, 120, 30);
        runtime
            .block_on(handle_key_press(&mut output_app, KeyCode::Char('k')))
            .expect("K should scroll output older");
        let output_after = render_grid(&output_app, 120, 30);

        assert_render_invariants(&output_before, 120, 30);
        assert_render_invariants(&output_after, 120, 30);
        for needle in ["Output", "Latest"] {
            assert_eq!(
                text_position(&output_before, needle),
                text_position(&output_after, needle),
                "{needle} should stay anchored while Output scrolls:\nbefore:\n{}\n\nafter:\n{}",
                screen_text(&output_before),
                screen_text(&output_after)
            );
        }
        assert_eq!(
            panel_border_signature(&output_before),
            panel_border_signature(&output_after),
            "Output scrolling must not move overlay chrome"
        );
        let output_scrolled = screen_text(&output_after);
        assert!(output_scrolled.contains("verify release archive contents"));
        assert!(!output_scrolled.contains("final archive checksum sent"));
    }

    #[test]
    fn usability_scroll_extremes_round_trip_without_artifacts() {
        let output = vec![
            "scroll 01",
            "scroll 02",
            "scroll 03",
            "scroll 04",
            "scroll 05",
            "scroll 06",
            "scroll 07",
            "scroll 08",
            "scroll 09",
            "scroll 10",
            "scroll 11",
            "scroll 12",
            "scroll 13",
            "scroll 14",
            "scroll 15",
            "scroll 16",
            "scroll 17",
            "scroll 18",
            "scroll 19",
            "scroll 20",
            "scroll 21",
            "scroll 22",
            "scroll 23",
            "scroll 24",
            "scroll 25",
            "scroll 26",
            "scroll 27",
            "scroll 28",
            "scroll 29",
            "scroll 30",
            "scroll 31",
            "scroll 32",
            "scroll 33",
            "scroll 34",
            "scroll 35",
            "scroll 36",
        ];
        let sizes = [(100, 24), (120, 30), (132, 30), (82, 24), (100, 14)];

        for (width, height) in sizes {
            let mut details =
                app_with_panes(vec![sample_pane("bash")], vec![("%1", output.clone())]);
            details.cycle_panel_focus();
            let details_bottom = render_grid(&details, width, height);
            for _ in 0..80 {
                details.select_previous_pane();
            }
            let details_top = render_grid(&details, width, height);
            for _ in 0..80 {
                details.select_next_pane();
            }
            let details_recovered = render_grid(&details, width, height);

            assert_render_invariants(&details_bottom, width, height);
            assert_render_invariants(&details_top, width, height);
            assert_render_invariants(&details_recovered, width, height);
            assert_eq!(
                text_position(&details_bottom, "Details"),
                text_position(&details_top, "Details"),
                "Details heading should stay anchored while scrolling at {width}x{height}"
            );
            assert_eq!(
                panel_border_signature(&details_bottom),
                panel_border_signature(&details_top),
                "Details borders should stay anchored while scrolling at {width}x{height}"
            );
            assert_ne!(
                normalize_relative_ages(details_top.clone()),
                normalize_relative_ages(details_bottom.clone()),
                "K should visibly move Details to older content at {width}x{height}"
            );
            assert_eq!(
                normalize_relative_ages(details_recovered),
                normalize_relative_ages(details_bottom),
                "J should recover the newest Details viewport at {width}x{height}"
            );

            let mut output_app =
                app_with_panes(vec![sample_pane("bash")], vec![("%1", output.clone())]);
            let runtime = tokio::runtime::Runtime::new().expect("runtime should build");
            runtime
                .block_on(handle_key_press(&mut output_app, KeyCode::Enter))
                .expect("enter should open output");
            let output_bottom = render_grid(&output_app, width, height);
            for _ in 0..80 {
                runtime
                    .block_on(handle_key_press(&mut output_app, KeyCode::Char('k')))
                    .expect("K should scroll older output");
            }
            let output_top = render_grid(&output_app, width, height);
            for _ in 0..80 {
                runtime
                    .block_on(handle_key_press(&mut output_app, KeyCode::Char('j')))
                    .expect("J should scroll newer output");
            }
            let output_recovered = render_grid(&output_app, width, height);

            assert_render_invariants(&output_bottom, width, height);
            assert_render_invariants(&output_top, width, height);
            assert_render_invariants(&output_recovered, width, height);
            for needle in ["Output", "Latest"] {
                assert_eq!(
                    text_position(&output_bottom, needle),
                    text_position(&output_top, needle),
                    "{needle} should stay anchored while scrolling at {width}x{height}"
                );
            }
            assert_eq!(
                panel_border_signature(&output_bottom),
                panel_border_signature(&output_top),
                "Output borders should stay anchored while scrolling at {width}x{height}"
            );
            assert_ne!(
                normalize_relative_ages(output_top.clone()),
                normalize_relative_ages(output_bottom.clone()),
                "K should visibly move Output to older content at {width}x{height}"
            );
            assert_eq!(
                normalize_relative_ages(output_recovered),
                normalize_relative_ages(output_bottom),
                "J should recover the newest Output viewport at {width}x{height}"
            );
        }
    }

    #[test]
    fn usability_filtering_from_scrolled_output_shows_the_new_pane_from_the_top() {
        let mut first = sample_pane("bash");
        first.id = String::from("%1");
        first.window_name = String::from("alpha");

        let mut second = sample_pane("codex");
        second.id = String::from("%2");
        second.window_id = String::from("@2");
        second.window_name = String::from("beta");
        second.active = false;
        second.pane_index = 1;

        let mut app = app_with_panes(
            vec![first, second],
            vec![
                (
                    "%1",
                    vec![
                        "step 01 plan",
                        "step 02 fetch",
                        "step 03 build",
                        "step 04 test",
                        "step 05 verify",
                        "step 06 lint",
                        "step 07 render",
                        "step 08 inspect",
                        "step 09 patch",
                        "step 10 unit",
                        "step 11 ux",
                        "step 12 live",
                        "step 13 perf",
                        "step 14 ci",
                        "step 15 review",
                        "step 16 package",
                        "step 17 smoke",
                        "step 18 bless",
                        "step 19 audit",
                        "step 20 done",
                    ],
                ),
                ("%2", vec!["Working on beta task"]),
            ],
        );
        let runtime = tokio::runtime::Runtime::new().expect("runtime should build");
        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Enter))
            .expect("enter should open output without tmux");
        app.select_next_pane();
        app.select_next_pane();

        app.begin_search();
        for ch in "beta".chars() {
            app.push_search_char(ch);
        }
        app.finish_search();

        let lines = render_grid(&app, 110, 18);
        let screen = screen_text(&lines);

        assert_render_invariants(&lines, 110, 18);
        assert!(app.is_details_panel_focused());
        assert!(screen.contains("Output"), "{screen}");
        assert!(screen.contains("demo / beta"), "{screen}");
        assert!(screen.contains("Working on beta task"), "{screen}");
        assert!(
            !screen.contains("step 03 build"),
            "stale scrolled output from the previous pane should not remain visible:\n{screen}"
        );
    }

    #[test]
    fn usability_focus_and_selection_are_visible_without_stealing_the_footer() {
        let fixture = panel_fixture("selected_waiting_panel");
        let mut app = app_from_panel_fixture(&fixture);
        let theme = Theme::from_preset(app.theme_preset());

        let assert_visible_state = |app: &App, label: &str| {
            let buffer = render_buffer(app, 120, 18);
            let lines = buffer_grid_lines(&buffer);
            let screen = screen_text(&lines);
            assert_render_invariants(&lines, 120, 18);
            assert_screen_has_one_line_chrome(label, &lines);
            assert_no_low_value_copy(label, &lines);

            let selected_row_y = lines
                .iter()
                .position(|line| line.contains(">! demo/agents"))
                .expect("selected fleet row should render");
            let selected = buffer_cell_in_line(
                &buffer,
                &lines[selected_row_y],
                selected_row_y,
                "demo/agents",
            );
            assert!(
                selected.fg != selected.bg || selected.modifier.contains(Modifier::REVERSED),
                "{label} selected fleet text needs contrast or reverse video:\n{screen}"
            );
            assert!(
                selected.modifier.contains(Modifier::BOLD),
                "{label} selected fleet row should be visibly selected:\n{screen}"
            );

            let footer = lines.last().expect("footer should render");
            assert!(footer.contains("? help"), "{label}\n{screen}");
            assert!(footer.contains(": reply"), "{label}\n{screen}");
            assert!(!footer.contains("focused"), "{label}\n{screen}");
        };

        assert_visible_state(&app, "fleet focus");
        let buffer = render_buffer(&app, 120, 18);
        assert_eq!(
            buffer.cell((0, 1)).expect("fleet border should exist").fg,
            theme.accent
        );

        app.cycle_panel_focus();
        assert_visible_state(&app, "details focus");
        let buffer = render_buffer(&app, 120, 18);
        assert_eq!(
            buffer
                .cell((66, 1))
                .expect("details border should exist")
                .fg,
            theme.accent
        );
    }

    #[test]
    fn row_tone_styles_match_alert_and_targeted_states() {
        let mut first = sample_pane("bash");
        first.id = String::from("%1");
        first.window_name = String::from("idle");

        let mut second = sample_pane("bash");
        second.id = String::from("%2");
        second.window_name = String::from("alert");
        second.active = false;
        second.pane_index = 1;

        let mut third = sample_pane("claude");
        third.id = String::from("%3");
        third.window_id = String::from("@3");
        third.window_name = String::from("waiting");
        third.active = false;
        third.pane_index = 2;

        let app = app_with_panes(
            vec![first, second, third],
            vec![
                ("%1", vec!["idle"]),
                ("%2", vec!["error: command failed"]),
                ("%3", vec!["Waiting for approval. Continue?"]),
            ],
        );
        let theme = Theme::from_preset(app.theme_preset());
        let buffer = render_buffer(&app, 120, 18);
        let lines = buffer_grid_lines(&buffer);

        let alert = buffer_cell(&buffer, &lines, "demo/alert");
        assert_eq!(alert.fg, theme.danger);
        assert!(alert.modifier.contains(Modifier::BOLD));

        let waiting = buffer_cell(&buffer, &lines, "demo/waiting");
        assert_eq!(waiting.fg, theme.warning);
        assert!(waiting.modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn attention_rows_keep_attention_style_even_when_targeted() {
        let mut waiting = sample_pane("claude");
        waiting.id = String::from("%1");
        waiting.window_name = String::from("approval");

        let mut working = sample_pane("codex");
        working.id = String::from("%2");
        working.window_id = String::from("@2");
        working.window_name = String::from("build");
        working.active = false;
        working.pane_index = 1;

        let mut app = app_with_panes(
            vec![waiting, working],
            vec![
                ("%1", vec!["Waiting for approval. Continue?"]),
                ("%2", vec!["building release"]),
            ],
        );
        app.toggle_selected_mark();
        app.select_next_pane();

        let theme = Theme::from_preset(app.theme_preset());
        let buffer = render_buffer(&app, 120, 18);
        let lines = buffer_grid_lines(&buffer);
        let screen = screen_text(&lines);

        assert_render_invariants(&lines, 120, 18);
        assert!(
            screen.contains("! demo/approval"),
            "targeted attention row should keep the attention marker:\n{screen}"
        );

        let row = buffer_cell(&buffer, &lines, "demo/approval");
        assert_eq!(row.fg, theme.warning, "{screen}");
        assert!(
            row.modifier.contains(Modifier::BOLD),
            "targeted attention row should remain visually urgent:\n{screen}"
        );
        assert_ne!(
            row.fg, theme.success,
            "send-list styling must not mask a pane that needs attention:\n{screen}"
        );
    }

    #[test]
    fn quiet_rows_are_visually_deemphasized() {
        let mut first = sample_pane("bash");
        first.id = String::from("%1");
        first.window_name = String::from("idle");

        let mut second = sample_pane("bash");
        second.id = String::from("%2");
        second.window_name = String::from("run");
        second.active = false;
        second.pane_index = 1;

        let mut app = app_with_panes(
            vec![first, second],
            vec![
                ("%1", vec!["done"]),
                ("%2", vec!["building release artifacts"]),
            ],
        );
        app.select_next_pane();
        let theme = Theme::from_preset(app.theme_preset());
        let buffer = render_buffer(&app, 120, 18);
        let lines = buffer_grid_lines(&buffer);

        let quiet = buffer_cell(&buffer, &lines, "demo/idle");
        let active = buffer_cell(&buffer, &lines, "demo/run");

        assert_eq!(quiet.fg, theme.muted);
        assert!(quiet.modifier.contains(Modifier::DIM));
        assert_eq!(active.fg, theme.text);
        assert!(!active.modifier.contains(Modifier::DIM));
    }

    #[test]
    fn targeted_rows_use_success_style_when_not_selected() {
        let mut first = sample_pane("bash");
        first.id = String::from("%1");
        first.window_name = String::from("idle");

        let mut second = sample_pane("bash");
        second.id = String::from("%2");
        second.window_name = String::from("mid");
        second.active = false;
        second.pane_index = 1;

        let mut third = sample_pane("bash");
        third.id = String::from("%3");
        third.window_name = String::from("target");
        third.active = false;
        third.pane_index = 2;

        let mut app = app_with_panes(
            vec![first, second, third],
            vec![
                ("%1", vec!["building..."]),
                ("%2", vec!["building..."]),
                ("%3", vec!["building..."]),
            ],
        );
        app.select_next_pane();
        app.select_next_pane();
        app.toggle_selected_mark();
        app.select_previous_pane();
        app.select_previous_pane();

        let theme = Theme::from_preset(app.theme_preset());
        let buffer = render_buffer(&app, 120, 18);
        let lines = buffer_grid_lines(&buffer);

        let targeted = buffer_cell(&buffer, &lines, "demo/target");
        assert_eq!(targeted.fg, theme.success);
        assert!(targeted.modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn section_heading_and_overlay_styles_use_accent_consistently() {
        let fixture = panel_fixture("live_tail_with_summary_and_raw_tail");
        let app = app_from_panel_fixture(&fixture);
        let theme = Theme::from_preset(app.theme_preset());
        let buffer = render_buffer(&app, 110, 18);
        let lines = buffer_grid_lines(&buffer);

        let output_y = line_index(&lines, "┌Output");
        let output_title = buffer_cell_in_line(&buffer, &lines[output_y], output_y, "Output");
        assert_eq!(output_title.fg, theme.accent);
        assert!(output_title.modifier.contains(Modifier::BOLD));

        let summary = buffer_cell(&buffer, &lines, "Summary");
        assert_eq!(summary.fg, theme.accent);
        assert!(summary.modifier.contains(Modifier::BOLD));

        let first_body = buffer_cell(&buffer, &lines, "demo / agents");
        assert_eq!(first_body.fg, theme.text);
        assert!(first_body.modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn selected_panel_uses_semantic_value_colors_for_state_blocker_and_next() {
        let fixture = panel_fixture("selected_waiting_panel");
        let app = app_from_panel_fixture(&fixture);
        let theme = Theme::from_preset(app.theme_preset());
        let buffer = render_buffer(&app, 120, 18);
        let lines = buffer_grid_lines(&buffer);
        let state_y = line_index(&lines, "State: Waiting   Tool: Claude Code");
        let waiting = buffer_cell_in_line(&buffer, &lines[state_y], state_y, "Waiting");
        assert_eq!(waiting.fg, theme.warning);

        let blocker_y = line_index(&lines, "Blocked: network access");
        let blocker = buffer_cell_in_line(&buffer, &lines[blocker_y], blocker_y, "network access");
        assert_eq!(blocker.fg, theme.warning);

        let next_y = line_index(&lines, "Action: : reply");
        let next = buffer_cell_in_line(&buffer, &lines[next_y], next_y, "reply");
        assert_eq!(next.fg, theme.accent);
    }

    #[test]
    fn long_labels_and_paths_do_not_break_responsive_layout() {
        let mut pane = sample_pane("node");
        pane.session_name = String::from("very-long-session-name-for-agent-fleet");
        pane.window_name = String::from("ridiculously-long-window-name-for-review");
        pane.current_path = String::from(
            "/workspace/muxboard/some/really/long/path/that/should/not/blow/up/the/layout",
        );

        let app = app_with_panes(
            vec![pane],
            vec![(
                "%1",
                vec!["codex", "STATUS=running | BLOCKER=none | NEXT=write tests"],
            )],
        );

        for &(width, height) in &[(68, 16), (84, 16), (120, 18), (140, 24)] {
            let lines = render_grid(&app, width, height);
            let screen = screen_text(&lines);
            assert_render_invariants(&lines, width, height);
            assert!(screen.contains("muxboard"), "{width}x{height}\n{screen}");
            assert!(screen.contains("write tests"), "{width}x{height}\n{screen}");
            assert!(screen.contains("? help"), "{width}x{height}\n{screen}");
        }
    }

    #[test]
    fn overlay_rect_stays_in_bounds_across_terminal_matrix() {
        for &(width, height) in &[
            (60_u16, 12_u16),
            (68_u16, 14_u16),
            (80_u16, 16_u16),
            (96_u16, 18_u16),
            (120_u16, 24_u16),
            (140_u16, 28_u16),
        ] {
            let body = Rect::new(0, 1, width, height.saturating_sub(2));
            for &(title, min_width, min_height) in &[
                ("Help", 24, 6),
                ("More", 24, 6),
                ("Output", 24, 6),
                ("Browse", 24, 6),
            ] {
                let lines = vec![
                    String::from("first line"),
                    String::from("second line"),
                    String::from("third line"),
                ];
                let rect = overlay_rect(body, title, &lines);
                assert!(rect.x >= body.x, "{title} {width}x{height}: {rect:?}");
                assert!(rect.y >= body.y, "{title} {width}x{height}: {rect:?}");
                assert!(
                    rect.x.saturating_add(rect.width) <= body.x.saturating_add(body.width),
                    "{title} {width}x{height}: {rect:?}"
                );
                assert!(
                    rect.y.saturating_add(rect.height) <= body.y.saturating_add(body.height),
                    "{title} {width}x{height}: {rect:?}"
                );
                assert!(
                    rect.width >= min_width,
                    "{title} {width}x{height}: {rect:?}"
                );
                assert!(
                    rect.height >= min_height,
                    "{title} {width}x{height}: {rect:?}"
                );
            }
        }
    }

    #[test]
    fn selected_layout_survives_resize_sequence_without_losing_core_wayfinding() {
        let fixture = panel_fixture("selected_waiting_panel");
        let app = app_from_panel_fixture(&fixture);

        for &(width, height) in &[
            (140, 24),
            (120, 20),
            (96, 18),
            (84, 16),
            (70, 16),
            (68, 14),
            (84, 16),
            (120, 20),
        ] {
            let lines = render_grid(&app, width, height);
            let screen = screen_text(&lines);
            assert_render_invariants(&lines, width, height);
            assert!(screen.contains("muxboard"), "{width}x{height}\n{screen}");
            assert!(screen.contains("Fleet"), "{width}x{height}\n{screen}");
            assert!(screen.contains("? help"), "{width}x{height}\n{screen}");
            assert!(
                screen.contains("Details") || screen.contains("Output") || screen.contains("Send"),
                "{width}x{height}\n{screen}"
            );
        }
    }

    #[test]
    fn dense_fleet_layout_holds_across_resize_sequence() {
        let panes = (0..12)
            .map(|index| {
                let mut pane = sample_pane(if index % 3 == 0 { "node" } else { "bash" });
                pane.id = format!("%{}", index + 1);
                pane.session_id = format!("${}", index / 6);
                pane.session_name = format!("ops-{}", index / 6);
                pane.window_id = format!("@{}", index / 2);
                pane.window_name = if index % 2 == 0 {
                    String::from("agents")
                } else {
                    String::from("workers")
                };
                pane.pane_index = index % 2;
                pane.active = index == 0;
                pane
            })
            .collect::<Vec<_>>();
        let runtimes = (0..12)
            .map(|index| {
                let lines = match index % 4 {
                    0 => vec!["codex", "STATUS=running | BLOCKER=none | NEXT=write tests"],
                    1 => vec!["Waiting for approval. Continue?"],
                    2 => vec!["error: command failed"],
                    _ => vec!["done"],
                };
                (format!("%{}", index + 1), lines)
            })
            .collect::<Vec<_>>();
        let runtime_refs = runtimes
            .iter()
            .map(|(pane_id, lines)| (pane_id.as_str(), lines.to_vec()))
            .collect::<Vec<_>>();
        let app = app_with_panes(panes, runtime_refs);

        for &(width, height) in &[(68, 16), (84, 16), (96, 18), (120, 18), (140, 24)] {
            let lines = render_grid(&app, width, height);
            let screen = screen_text(&lines);
            assert_render_invariants(&lines, width, height);
            assert!(screen.contains("Fleet"), "{width}x{height}\n{screen}");
            assert!(screen.contains("Details"), "{width}x{height}\n{screen}");
            assert!(
                screen.contains("ops-0") || screen.contains("ops-1"),
                "{width}x{height}\n{screen}"
            );
            assert!(screen.contains("? help"), "{width}x{height}\n{screen}");
        }
    }

    #[test]
    fn tiny_terminal_preserves_core_chrome_for_selected_and_output_modes() {
        let selected = app_from_panel_fixture(&panel_fixture("selected_waiting_panel"));
        let output = app_from_panel_fixture(&panel_fixture("live_tail_with_summary_and_raw_tail"));

        for app in [&selected, &output] {
            let lines = render_grid(app, 60, 12);
            let screen = screen_text(&lines);
            assert_render_invariants(&lines, 60, 12);
            assert!(screen.contains("muxboard"), "{screen}");
            assert!(screen.contains("? help"), "{screen}");
            assert!(screen.contains("┌"), "{screen}");
        }
    }

    #[test]
    fn transition_sequence_preserves_wayfinding_during_multi_pane_attention_churn() {
        let mut build = sample_pane("codex");
        build.id = String::from("%1");
        build.window_name = String::from("build");

        let mut review = sample_pane("claude");
        review.id = String::from("%2");
        review.window_name = String::from("review");
        review.active = false;
        review.pane_index = 1;

        let mut ship = sample_pane("bash");
        ship.id = String::from("%3");
        ship.window_name = String::from("ship");
        ship.active = false;
        ship.pane_index = 2;

        let phases = [
            (
                vec![
                    ("%1", vec!["Waiting for approval. Continue?"]),
                    (
                        "%2",
                        vec!["STATUS=running | BLOCKER=none | NEXT=review diff"],
                    ),
                    ("%3", vec!["done"]),
                ],
                "Running",
                "1 needs you",
            ),
            (
                vec![
                    (
                        "%1",
                        vec!["STATUS=running | BLOCKER=none | NEXT=compile fixes"],
                    ),
                    ("%2", vec!["Waiting for approval. Continue?"]),
                    ("%3", vec!["error: command failed"]),
                ],
                "Waiting",
                "2 need you",
            ),
            (
                vec![
                    ("%1", vec!["done"]),
                    ("%2", vec!["error: network failed"]),
                    ("%3", vec!["Waiting for approval. Continue?"]),
                ],
                "Error",
                "2 need you",
            ),
            (
                vec![
                    ("%1", vec!["done"]),
                    (
                        "%2",
                        vec!["STATUS=running | BLOCKER=none | NEXT=merge branch"],
                    ),
                    ("%3", vec!["done"]),
                ],
                "Running",
                "1 working",
            ),
        ];

        for (runtimes, selected_status, attention_summary) in phases {
            for &(width, height) in &[(96, 18), (84, 16), (68, 14)] {
                let mut app = app_with_panes(
                    vec![build.clone(), review.clone(), ship.clone()],
                    runtimes.clone(),
                );
                app.cycle_sort_mode();
                app.cycle_sort_mode();
                app.select_next_pane();
                let lines = render_grid(&app, width, height);
                let screen = screen_text(&lines);

                assert_render_invariants(&lines, width, height);
                assert!(screen.contains("muxboard"), "{width}x{height}\n{screen}");
                assert!(screen.contains("Fleet"), "{width}x{height}\n{screen}");
                assert!(screen.contains("Details"), "{width}x{height}\n{screen}");
                assert!(screen.contains("demo/review"), "{width}x{height}\n{screen}");
                assert!(
                    screen.contains(selected_status),
                    "{width}x{height}\n{screen}"
                );
                assert!(
                    screen.contains(attention_summary),
                    "{width}x{height}\n{screen}"
                );
                assert!(screen.contains("? help"), "{width}x{height}\n{screen}");
            }
        }
    }

    #[test]
    fn cuj_triage_fleet_makes_attention_and_next_step_obvious_across_sizes() {
        let mut waiting = sample_pane("claude");
        waiting.id = String::from("%1");
        waiting.window_name = String::from("approval");

        let mut running = sample_pane("node");
        running.id = String::from("%2");
        running.window_name = String::from("build");
        running.active = false;
        running.pane_index = 1;

        let mut error = sample_pane("bash");
        error.id = String::from("%3");
        error.window_name = String::from("deploy");
        error.active = false;
        error.pane_index = 2;

        let mut done = sample_pane("bash");
        done.id = String::from("%4");
        done.window_name = String::from("done");
        done.active = false;
        done.pane_index = 3;

        let app = app_with_panes(
            vec![waiting, running, error, done],
            vec![
                ("%1", vec!["Waiting for approval. Continue?"]),
                (
                    "%2",
                    vec!["codex", "STATUS=running | BLOCKER=none | NEXT=write tests"],
                ),
                ("%3", vec!["error: command failed"]),
                ("%4", vec!["done"]),
            ],
        );

        for &(width, height) in &[(80, 16), (100, 20), (120, 24)] {
            let lines = render_grid(&app, width, height);
            let screen = screen_text(&lines);

            assert_render_invariants(&lines, width, height);
            assert!(screen.contains("muxboard"), "{width}x{height}\n{screen}");
            assert!(screen.contains("Fleet"), "{width}x{height}\n{screen}");
            assert!(screen.contains("Details"), "{width}x{height}\n{screen}");
            assert!(screen.contains("2 need you"), "{width}x{height}\n{screen}");
            assert!(
                screen.contains("Action: : reply"),
                "{width}x{height}\n{screen}"
            );
            assert!(screen.contains("? help"), "{width}x{height}\n{screen}");
            assert!(!screen.contains("STATUS="), "{width}x{height}\n{screen}");
            assert!(
                line_index(&lines, "State:") < line_index(&lines, "Action:"),
                "{width}x{height}\n{screen}"
            );
        }
    }

    #[test]
    fn cuj_inspect_agent_moves_from_distilled_card_to_intentional_output_layer() {
        let pane = sample_pane("node");
        let mut app = app_with_panes(
            vec![pane],
            vec![(
                "%1",
                vec![
                    "codex",
                    "STATUS=running | BLOCKER=none | NEXT=write tests",
                    "building release artifacts",
                ],
            )],
        );

        for &(width, height) in &[(80, 16), (100, 20), (120, 24)] {
            let lines = render_grid(&app, width, height);
            let screen = screen_text(&lines);
            assert_render_invariants(&lines, width, height);
            assert!(screen.contains("Details"), "{width}x{height}\n{screen}");
            assert!(
                screen.contains("Now: write tests"),
                "{width}x{height}\n{screen}"
            );
            assert!(!screen.contains("STATUS="), "{width}x{height}\n{screen}");
            assert!(!screen.contains("│  codex"), "{width}x{height}\n{screen}");
        }

        app.cycle_context_pane();
        for &(width, height) in &[(80, 16), (100, 20), (120, 24)] {
            let lines = render_grid(&app, width, height);
            let screen = screen_text(&lines);
            assert_render_invariants(&lines, width, height);
            assert!(screen.contains("Output"), "{width}x{height}\n{screen}");
            assert!(screen.contains("Summary"), "{width}x{height}\n{screen}");
            assert!(screen.contains("write tests"), "{width}x{height}\n{screen}");
            assert!(screen.contains("Latest"), "{width}x{height}\n{screen}");
            assert!(
                screen.contains("building release artifacts"),
                "{width}x{height}\n{screen}"
            );
            assert!(!screen.contains("STATUS="), "{width}x{height}\n{screen}");
            assert!(line_index(&lines, "Summary") < line_index(&lines, "Latest"));
        }
    }

    #[test]
    fn cuj_act_on_waiting_agent_keeps_action_and_recovery_paths_visible() {
        let pane = sample_pane("claude");
        let mut app = app_with_panes(vec![pane], vec![("%1", vec!["Press Enter to continue."])]);

        for &(width, height) in &[(80, 16), (100, 20), (120, 24)] {
            let lines = render_grid(&app, width, height);
            let screen = screen_text(&lines);
            assert_render_invariants(&lines, width, height);
            assert!(
                screen.contains("State: Waiting"),
                "{width}x{height}\n{screen}"
            );
            assert!(
                screen.contains("Action: A continue"),
                "{width}x{height}\n{screen}"
            );
            assert!(
                screen.contains("Also: : send"),
                "{width}x{height}\n{screen}"
            );
            assert!(screen.contains("? help"), "{width}x{height}\n{screen}");
            if width >= 96 {
                assert!(screen.contains("A continue"), "{width}x{height}\n{screen}");
            }
            assert!(
                screen.contains("G show") || screen.contains(". more"),
                "{width}x{height}\n{screen}"
            );
            assert!(
                line_index(&lines, "Action:") < line_index(&lines, "Also:"),
                "{width}x{height}\n{screen}"
            );
        }

        app.open_action_menu();
        let lines = render_grid(&app, 100, 20);
        let screen = screen_text(&lines);
        assert_render_invariants(&lines, 100, 20);
        assert!(screen.contains("More"), "{screen}");
        assert!(screen.contains("Action:"), "{screen}");
        assert!(screen.contains("I continue waiting"), "{screen}");
        assert!(
            line_index(&lines, "I continue waiting") < line_index(&lines, "Z zoom pane"),
            "More should put the obvious waiting action before secondary pane tools:\n{screen}"
        );
        assert!(
            screen.contains("Esc") && screen.contains("close"),
            "{screen}"
        );
    }

    #[test]
    fn cuj_send_to_multiple_agents_surfaces_targets_confirm_command_and_safe_confirm() {
        let app = app_from_panel_fixture(&panel_fixture("send_panel_confirm_dispatch"));

        for &(width, height) in &[(80, 16), (100, 20), (120, 24)] {
            let lines = render_grid(&app, width, height);
            let screen = screen_text(&lines);
            assert_render_invariants(&lines, width, height);
            assert!(screen.contains("Send"), "{width}x{height}\n{screen}");
            assert!(screen.contains("Review send"), "{width}x{height}\n{screen}");
            assert!(screen.contains("Targets"), "{width}x{height}\n{screen}");
            assert!(
                screen.contains("fleet triage (2 panes)"),
                "{width}x{height}\n{screen}"
            );
            assert!(
                screen.contains("Text: continue"),
                "{width}x{height}\n{screen}"
            );
            assert!(screen.contains("Enter send"), "{width}x{height}\n{screen}");
            assert!(
                screen.contains("Esc cancel") || screen.contains("Esc to cancel"),
                "{width}x{height}\n{screen}"
            );
            assert!(line_index(&lines, "│ To:") < line_index(&lines, "│ Text:"));
            assert!(line_index(&lines, "│ Targets") < line_index(&lines, "│   demo"));
        }
    }

    #[test]
    fn cuj_recover_with_search_browse_and_clear_scope_preserves_wayfinding() {
        let mut first = sample_pane("codex");
        first.id = String::from("%1");
        first.window_id = String::from("@1");
        first.window_name = String::from("agents");

        let mut second = sample_pane("bash");
        second.id = String::from("%2");
        second.window_id = String::from("@2");
        second.window_name = String::from("shells");
        second.active = false;
        second.pane_index = 1;

        let mut app = app_with_panes(
            vec![first, second],
            vec![
                (
                    "%1",
                    vec!["codex", "STATUS=running | BLOCKER=none | NEXT=write tests"],
                ),
                ("%2", vec!["idle"]),
            ],
        );

        app.begin_search();
        for ch in "shells".chars() {
            app.push_search_char(ch);
        }
        for &(width, height) in &[(80, 16), (100, 20), (120, 24)] {
            let lines = render_grid(&app, width, height);
            let screen = screen_text(&lines);
            assert_render_invariants(&lines, width, height);
            assert!(
                screen.contains("Searching for `shells`"),
                "{width}x{height}\n{screen}"
            );
            assert!(
                screen.contains("Enter apply") || screen.contains("Enter to apply"),
                "{width}x{height}\n{screen}"
            );
            assert!(
                screen.contains("Esc cancel") || screen.contains("Esc to cancel"),
                "{width}x{height}\n{screen}"
            );
            assert!(!screen.contains("? help"), "{width}x{height}\n{screen}");
        }

        app.finish_search();
        let lines = render_grid(&app, 100, 20);
        let screen = screen_text(&lines);
        assert_render_invariants(&lines, 100, 20);
        assert!(screen.contains("search: shells"), "{screen}");
        assert!(screen.contains("demo/shells"), "{screen}");

        for _ in 0..3 {
            app.cycle_context_pane();
        }
        let lines = render_grid(&app, 100, 20);
        let screen = screen_text(&lines);
        assert_render_invariants(&lines, 100, 20);
        assert!(screen.contains("Browse"), "{screen}");
        assert!(screen.contains("shells"), "{screen}");

        app.clear_view_scope();
        assert!(app.footer_line_for_width(120).contains("? help"));
    }

    #[test]
    fn dense_fleet_with_competing_attention_keeps_current_selection_visible() {
        let panes = (0..10)
            .map(|index| {
                let mut pane = sample_pane(if index % 2 == 0 { "claude" } else { "node" });
                pane.id = format!("%{}", index + 1);
                pane.window_id = format!("@{}", index + 1);
                pane.window_name = format!("job-{}", index + 1);
                pane.pane_index = 0;
                pane.active = index == 0;
                pane
            })
            .collect::<Vec<_>>();
        let runtimes = vec![
            ("%1", vec!["done"]),
            (
                "%2",
                vec!["STATUS=running | BLOCKER=none | NEXT=write tests"],
            ),
            ("%3", vec!["Waiting for approval. Continue?"]),
            ("%4", vec!["error: command failed"]),
            ("%5", vec!["done"]),
            (
                "%6",
                vec!["STATUS=running | BLOCKER=none | NEXT=sync results"],
            ),
            ("%7", vec!["Waiting for approval. Continue?"]),
            ("%8", vec!["error: network failed"]),
            ("%9", vec!["done"]),
            (
                "%10",
                vec!["STATUS=running | BLOCKER=none | NEXT=ship release"],
            ),
        ];

        let mut app = app_with_panes(panes, runtimes);
        app.cycle_sort_mode();
        app.cycle_sort_mode();
        for _ in 0..7 {
            app.select_next_pane();
        }

        for &(width, height) in &[(96, 18), (84, 16), (68, 14)] {
            let lines = render_grid(&app, width, height);
            let screen = screen_text(&lines);
            assert_render_invariants(&lines, width, height);
            assert!(screen.contains("Fleet"), "{width}x{height}\n{screen}");
            assert!(screen.contains("Details"), "{width}x{height}\n{screen}");
            assert!(screen.contains("demo/job-8"), "{width}x{height}\n{screen}");
            assert!(screen.contains("Error"), "{width}x{height}\n{screen}");
            assert!(screen.contains("4 need you"), "{width}x{height}\n{screen}");
        }
    }

    #[test]
    fn selected_attention_hierarchy_stays_explicit_under_dense_fleet_pressure() {
        let panes = (0..10)
            .map(|index| {
                let mut pane = sample_pane(if index % 2 == 0 { "claude" } else { "node" });
                pane.id = format!("%{}", index + 1);
                pane.window_id = format!("@{}", index + 1);
                pane.window_name = format!("job-{}", index + 1);
                pane.pane_index = 0;
                pane.active = index == 0;
                pane
            })
            .collect::<Vec<_>>();
        let runtimes = vec![
            ("%1", vec!["done"]),
            (
                "%2",
                vec!["STATUS=running | BLOCKER=none | NEXT=write tests"],
            ),
            ("%3", vec!["Waiting for approval. Continue?"]),
            ("%4", vec!["error: command failed"]),
            ("%5", vec!["done"]),
            (
                "%6",
                vec!["STATUS=running | BLOCKER=none | NEXT=sync results"],
            ),
            ("%7", vec!["Waiting for approval. Continue?"]),
            ("%8", vec!["error: network failed"]),
            ("%9", vec!["done"]),
            (
                "%10",
                vec!["STATUS=running | BLOCKER=none | NEXT=ship release"],
            ),
        ];

        let mut app = app_with_panes(panes, runtimes);
        app.cycle_sort_mode();
        app.cycle_sort_mode();
        for _ in 0..7 {
            app.select_next_pane();
        }

        for &(width, height) in &[(68, 14), (84, 16), (96, 18)] {
            let lines = render_grid(&app, width, height);
            let screen = screen_text(&lines);
            let selected_loc_y = *line_indices(&lines, "demo/job-8")
                .last()
                .expect("selected location should be visible");
            let status_y = line_index(&lines, "Error");
            let problem_y = line_index(&lines, "Problem:");
            let next_y = line_index(&lines, "Action:");
            let queue_positions = lines
                .iter()
                .enumerate()
                .filter_map(|(index, line)| line.contains("Queue: #2").then_some(index))
                .collect::<Vec<_>>();

            assert_render_invariants(&lines, width, height);
            assert!(screen.contains("Fleet"), "{width}x{height}\n{screen}");
            assert!(screen.contains("Details"), "{width}x{height}\n{screen}");
            assert!(screen.contains("demo/job-8"), "{width}x{height}\n{screen}");
            assert!(screen.contains("Error"), "{width}x{height}\n{screen}");
            assert!(screen.contains("Problem:"), "{width}x{height}\n{screen}");
            assert!(screen.contains("Action:"), "{width}x{height}\n{screen}");
            assert_eq!(status_y, selected_loc_y + 1, "{width}x{height}\n{screen}");
            assert_eq!(problem_y, status_y + 1, "{width}x{height}\n{screen}");
            assert_eq!(next_y, problem_y + 1, "{width}x{height}\n{screen}");
            if width >= 96 {
                let queue_y = *queue_positions
                    .first()
                    .expect("queue should survive on wider selected panels");
                assert_eq!(queue_y, next_y + 1, "{width}x{height}\n{screen}");
            } else {
                assert!(queue_positions.is_empty(), "{width}x{height}\n{screen}");
            }
        }
    }

    #[test]
    fn crowded_fleet_keeps_hottest_rows_at_the_top_of_the_visible_window() {
        let panes = (0..12)
            .map(|index| {
                let command = match index {
                    0 => "codex",
                    1 => "claude",
                    2 => "opencode",
                    3 => "aider",
                    4 => "gemini",
                    _ => "bash",
                };
                let mut pane = sample_pane(command);
                pane.id = format!("%{}", index + 1);
                pane.window_id = format!("@{}", index + 1);
                pane.window_name = format!("job-{}", index + 1);
                pane.pane_index = 0;
                pane.active = index == 0;
                pane
            })
            .collect::<Vec<_>>();
        let runtimes = vec![
            ("%1", vec!["error: command failed"]),
            ("%2", vec!["Waiting for approval. Continue?"]),
            ("%3", vec!["Waiting for approval. Continue?"]),
            (
                "%4",
                vec!["STATUS=running | BLOCKER=none | NEXT=review diff"],
            ),
            (
                "%5",
                vec!["STATUS=running | BLOCKER=none | NEXT=sync results"],
            ),
            ("%6", vec!["done"]),
            ("%7", vec!["done"]),
            ("%8", vec!["idle"]),
            ("%9", vec!["idle"]),
            ("%10", vec!["done"]),
            ("%11", vec!["idle"]),
            ("%12", vec!["done"]),
        ];
        let app = app_with_panes(panes, runtimes);

        for &(width, height) in &[(84, 16), (96, 18)] {
            let lines = render_grid(&app, width, height);
            let screen = screen_text(&lines);
            let row_error_y = line_index(&lines, "demo/job-1");
            let row_wait_one_y = line_index(&lines, "demo/job-2");
            let row_wait_two_y = line_index(&lines, "demo/job-3");
            let row_run_y = line_index(&lines, "demo/job-4");

            assert_render_invariants(&lines, width, height);
            assert!(screen.contains("Fleet"), "{width}x{height}\n{screen}");
            assert!(screen.contains("Details"), "{width}x{height}\n{screen}");
            assert!(screen.contains("3 need you"), "{width}x{height}\n{screen}");
            assert!(row_error_y < row_wait_one_y, "{width}x{height}\n{screen}");
            assert!(
                row_wait_one_y < row_wait_two_y,
                "{width}x{height}\n{screen}"
            );
            assert!(row_wait_two_y < row_run_y, "{width}x{height}\n{screen}");
        }
    }

    #[test]
    fn overview_overlay_keeps_selected_lane_visible_under_lane_pressure() {
        let mut codex = sample_pane("codex");
        codex.id = String::from("%1");
        codex.window_name = String::from("codex");

        let mut claude = sample_pane("claude");
        claude.id = String::from("%2");
        claude.window_name = String::from("claude");
        claude.active = false;

        let mut opencode = sample_pane("opencode");
        opencode.id = String::from("%3");
        opencode.window_name = String::from("opencode");
        opencode.active = false;

        let mut aider = sample_pane("aider");
        aider.id = String::from("%4");
        aider.window_name = String::from("aider");
        aider.active = false;

        let mut gemini = sample_pane("gemini");
        gemini.id = String::from("%5");
        gemini.window_name = String::from("gemini");
        gemini.active = false;

        let mut agent = sample_pane("node");
        agent.id = String::from("%6");
        agent.window_name = String::from("agent");
        agent.active = false;

        let mut app = app_with_panes(
            vec![codex, claude, opencode, aider, gemini, agent],
            vec![
                ("%1", vec!["error: command failed"]),
                ("%2", vec!["Waiting for approval. Continue?"]),
                ("%3", vec!["Waiting for approval. Continue?"]),
                ("%4", vec!["thinking..."]),
                ("%5", vec!["thinking..."]),
                ("%6", vec!["agent", "done"]),
            ],
        );
        app.cycle_sort_mode();
        app.cycle_sort_mode();
        for _ in 0..5 {
            app.select_next_pane();
        }
        for _ in 0..4 {
            app.cycle_context_pane();
        }

        let lines = render_grid(&app, 96, 18);
        let screen = screen_text(&lines);
        let lanes_y = line_index(&lines, "Lanes");
        let codex_y = line_index(&lines, "  codex:");
        let agent_y = line_index(&lines, "> agent:");

        assert_render_invariants(&lines, 96, 18);
        assert!(screen.contains("Command Center"), "{screen}");
        assert!(screen.contains("Lanes"), "{screen}");
        assert!(screen.contains("> agent:"), "{screen}");
        assert!(screen.contains("  codex:"), "{screen}");
        assert!(!screen.contains("  claude:"), "{screen}");
        assert!(!screen.contains("  opencode:"), "{screen}");
        assert!(!screen.contains("  aider:"), "{screen}");
        assert!(!screen.contains("  gemini:"), "{screen}");
        assert!(codex_y > lanes_y, "{screen}");
        assert!(agent_y > codex_y, "{screen}");
    }

    #[test]
    fn command_center_no_match_search_uses_visible_scope_for_status_counts() {
        let mut waiting = sample_pane("codex");
        waiting.id = String::from("%1");

        let mut running = sample_pane("claude");
        running.id = String::from("%2");
        running.window_id = String::from("@1");
        running.window_name = String::from("agents-2");
        running.active = false;
        running.pane_index = 1;

        let mut app = app_with_panes(
            vec![waiting, running],
            vec![
                ("%1", vec!["Waiting for approval. Continue?"]),
                ("%2", vec!["thinking..."]),
            ],
        );
        app.show_command_center();
        app.begin_search();
        for ch in "zzz-no-match".chars() {
            app.push_search_char(ch);
        }
        app.finish_search();

        let lines = render_grid(&app, 80, 16);
        let screen = screen_text(&lines);

        assert_render_invariants(&lines, 80, 16);
        assert!(screen.contains("Command Center"), "{screen}");
        assert!(screen.contains("no matches"), "{screen}");
        assert!(
            screen.contains("Action: backspace show all panes"),
            "{screen}"
        );
        assert!(!screen.contains("Send:"), "{screen}");
        assert!(!screen.contains("Start:"), "{screen}");
        assert!(!screen.contains("Needs you: none"), "{screen}");
        assert!(!screen.contains("Working: none"), "{screen}");
        assert!(!screen.contains("Needs you: 1 waiting"), "{screen}");
        assert!(!screen.contains("Working: 2 agents"), "{screen}");
    }

    #[test]
    fn command_center_working_count_excludes_waiting_agents() {
        let mut running_agent = sample_pane("claude");
        running_agent.id = String::from("%1");
        running_agent.window_name = String::from("alpha");

        let mut waiting_agent = sample_pane("codex");
        waiting_agent.id = String::from("%2");
        waiting_agent.window_name = String::from("blocked");
        waiting_agent.active = false;
        waiting_agent.pane_index = 1;

        let mut waiting_shell = sample_pane("zsh");
        waiting_shell.id = String::from("%3");
        waiting_shell.window_name = String::from("approval");
        waiting_shell.active = false;
        waiting_shell.pane_index = 2;

        let mut app = app_with_panes(
            vec![running_agent, waiting_agent, waiting_shell],
            vec![
                (
                    "%1",
                    vec!["STATUS=running | BLOCKER=none | NEXT=continue build"],
                ),
                (
                    "%2",
                    vec!["STATUS=waiting | BLOCKER=missing fixture | NEXT=need input"],
                ),
                ("%3", vec!["Proceed? [y/N]"]),
            ],
        );
        app.show_command_center();

        let lines = render_grid(&app, 80, 20);
        let screen = screen_text(&lines);

        assert_render_invariants(&lines, 80, 20);
        assert!(screen.contains("Command Center"), "{screen}");
        assert!(screen.contains("Needs you: 2 waiting"), "{screen}");
        assert!(screen.contains("Working: 1 agent"), "{screen}");
        assert!(!screen.contains("Working: 2 agents"), "{screen}");
    }

    #[test]
    fn command_center_attention_queue_shows_count_and_hidden_items() {
        let mut panes = Vec::new();
        let mut pane_ids = Vec::new();

        for index in 0..8 {
            let mut pane = sample_pane("codex");
            pane.id = format!("%{}", index + 1);
            pane.window_id = format!("@{index}");
            pane.window_name = format!("agent-{index}");
            pane.active = index == 0;
            pane.pane_index = index;
            pane_ids.push(pane.id.clone());
            panes.push(pane);
        }

        let runtimes = pane_ids
            .iter()
            .map(|id| (id.as_str(), vec!["Press Enter to continue."]))
            .collect::<Vec<_>>();
        let mut app = app_with_panes(panes, runtimes);
        app.show_command_center();

        let lines = render_grid(&app, 120, 22);
        let screen = screen_text(&lines);

        assert_render_invariants(&lines, 120, 22);
        assert!(screen.contains("Command Center"), "{screen}");
        assert!(screen.contains("Needs you: 8 waiting"), "{screen}");
        assert!(screen.contains("Queue (8)"), "{screen}");
        assert!(screen.contains("> continue demo / agent-0"), "{screen}");
        assert!(screen.contains("  continue demo / agent-5"), "{screen}");
        assert!(screen.contains("+ 2 more need you: continue"), "{screen}");
        assert!(
            line_index(&lines, "Queue (8)") < line_index(&lines, "> continue demo / agent-0"),
            "{screen}"
        );
        assert!(
            line_index(&lines, "  continue demo / agent-5")
                < line_index(&lines, "+ 2 more need you: continue"),
            "{screen}"
        );
        assert!(!screen.contains("agent-6"), "{screen}");
        assert!(!screen.contains("agent-7"), "{screen}");
        assert!(!screen.contains("STATUS="), "{screen}");
        assert!(!screen.contains("NEXT="), "{screen}");

        let compact_lines = render_grid(&app, 80, 14);
        let compact = screen_text(&compact_lines);

        assert_render_invariants(&compact_lines, 80, 14);
        assert!(compact.contains("Command Center"), "{compact}");
        assert!(compact.contains("Needs you: 8 waiting"), "{compact}");
        assert!(compact.contains("> continue demo / agent-0"), "{compact}");
        assert!(
            compact.contains("+ 2 more need you: continue"),
            "compact Command Center must not hide queue overflow:\n{compact}"
        );
        assert!(!compact.contains("agent-6"), "{compact}");
        assert!(!compact.contains("agent-7"), "{compact}");
    }

    #[test]
    fn command_center_attention_overflow_summarizes_hidden_action_types() {
        let mut panes = Vec::new();
        let mut runtimes = Vec::new();

        for index in 0..8 {
            let mut pane = sample_pane("codex");
            pane.id = format!("%{}", index + 1);
            pane.window_id = format!("@{index}");
            pane.window_name = format!("agent-{index}");
            pane.active = index == 0;
            pane.pane_index = index;
            let output = match index {
                6 => "Allow command? [y/n]",
                7 => "Waiting for leader to approve network access.",
                _ => "Press Enter to continue.",
            };
            runtimes.push((pane.id.clone(), vec![output.to_owned()]));
            panes.push(pane);
        }

        let runtime_refs = runtimes
            .iter()
            .map(|(id, lines)| (id.as_str(), lines.iter().map(String::as_str).collect()))
            .collect::<Vec<(&str, Vec<&str>)>>();
        let mut app = app_with_panes(panes, runtime_refs);
        app.show_command_center();

        let lines = render_grid(&app, 120, 22);
        let screen = screen_text(&lines);

        assert_render_invariants(&lines, 120, 22);
        assert!(screen.contains("Command Center"), "{screen}");
        assert!(screen.contains("Queue (8)"), "{screen}");
        assert!(screen.contains("> continue demo / agent-0"), "{screen}");
        assert!(
            screen.contains("+ 2 more need you: answer, reply"),
            "hidden queue rows should summarize what kind of attention remains:\n{screen}"
        );
        assert!(!screen.contains("agent-6"), "{screen}");
        assert!(!screen.contains("agent-7"), "{screen}");
    }

    #[test]
    fn usability_short_command_center_keeps_action_target_and_start_visible() {
        let fixture = panel_fixture("overview_panel_with_attention");
        let app = app_from_panel_fixture(&fixture);
        let lines = render_grid(&app, 80, 14);
        let screen = screen_text(&lines);

        assert_render_invariants(&lines, 80, 14);
        assert!(screen.contains("Command Center"), "{screen}");
        assert!(screen.contains("Action:"), "{screen}");
        assert!(screen.contains("Target:"), "{screen}");
        assert!(screen.contains("Start:"), "{screen}");
        assert!(screen.contains("> reply to demo / agents"), "{screen}");
        assert!(screen.contains("Lanes"), "{screen}");
        assert!(screen.contains("> codex: 1 pane | 1 waiting"), "{screen}");
        assert!(
            !screen.contains("Working:"),
            "short Command Center should spend scarce rows on actions before passive status:\n{screen}"
        );
        assert!(
            line_index(&lines, "Action:") < line_index(&lines, "Target:"),
            "{screen}"
        );
        assert!(
            line_index(&lines, "Target:") < line_index(&lines, "Start:"),
            "{screen}"
        );
        assert_eq!(
            line_index(&lines, "Lanes"),
            line_index(&lines, "> reply to demo / agents") + 1,
            "{screen}"
        );
        assert_eq!(
            line_index(&lines, "> codex:"),
            line_index(&lines, "Lanes") + 1,
            "{screen}"
        );
    }

    #[test]
    fn tiny_command_center_keeps_primary_actions_and_selection_visible() {
        let fixture = panel_fixture("overview_panel_with_attention");
        let app = app_from_panel_fixture(&fixture);
        let lines = render_grid(&app, 60, 12);
        let screen = screen_text(&lines);
        let footer = lines.last().expect("footer should render");

        assert_render_invariants(&lines, 60, 12);
        assert!(screen.contains("Command Center"), "{screen}");
        assert!(screen.contains("Action:"), "{screen}");
        assert!(screen.contains("Target:"), "{screen}");
        assert!(screen.contains("Start:"), "{screen}");
        assert!(screen.contains("Needs you:"), "{screen}");
        assert!(screen.contains("> reply to demo / agents"), "{screen}");
        assert!(
            line_index(&lines, "Action:") < line_index(&lines, "Target:"),
            "{screen}"
        );
        assert!(
            line_index(&lines, "Target:") < line_index(&lines, "Start:"),
            "{screen}"
        );
        assert!(
            line_index(&lines, "Start:") < line_index(&lines, "> reply to demo / agents"),
            "{screen}"
        );
        assert!(footer.contains(": reply"), "{screen}");
        assert!(footer.contains("Esc back"), "{screen}");
        assert!(
            !footer.contains("Enter output"),
            "Command Center footer should advertise its primary action, not an unrelated Details action:\n{screen}"
        );
    }

    #[test]
    fn usability_command_center_footer_matches_body_action_without_duplicates() {
        let mut waiting = app_with_panes(
            vec![sample_pane("codex")],
            vec![("%1", vec!["Press Enter to continue."])],
        );
        waiting.show_command_center();

        let mut error = app_with_panes(
            vec![sample_pane("codex")],
            vec![("%1", vec!["error: command failed"])],
        );
        error.show_command_center();

        let mut prompt = app_with_panes(
            vec![sample_pane("codex")],
            vec![("%1", vec!["Type your answer to continue."])],
        );
        prompt.show_command_center();

        let mut idle = app_with_panes(vec![sample_pane("codex")], vec![("%1", vec!["working"])]);
        idle.show_command_center();

        let mut marked = app_with_panes(vec![sample_pane("codex")], vec![("%1", vec!["working"])]);
        marked.toggle_selected_mark();
        marked.show_command_center();

        let mut lane = app_with_panes(vec![sample_pane("codex")], vec![("%1", vec!["working"])]);
        lane.toggle_fanout_mode();
        lane.show_command_center();

        let mut no_panes = app_with_panes(Vec::new(), vec![]);
        no_panes.show_command_center();

        for (name, app, body_action, footer_action, forbidden) in [
            (
                "waiting",
                waiting,
                "Action: A continue",
                "A continue",
                Some("Enter output"),
            ),
            ("error", error, "Action: Enter output", "Enter output", None),
            (
                "prompt",
                prompt,
                "Action: : reply",
                ": reply",
                Some(": send"),
            ),
            (
                "idle",
                idle,
                "Action: Enter output",
                "Enter output",
                Some(": send"),
            ),
            (
                "marked",
                marked,
                "Action: : send to the send list",
                ": send",
                None,
            ),
            ("lane", lane, "Action: : send", ": send", None),
            (
                "no panes",
                no_panes,
                "Action: start tmux panes, then R refresh",
                "R refresh",
                Some(": send"),
            ),
        ] {
            let lines = render_grid(&app, 96, 16);
            let screen = screen_text(&lines);
            let footer = lines.last().expect("footer should render");

            assert_render_invariants(&lines, 96, 16);
            assert!(screen.contains("Command Center"), "{name}\n{screen}");
            assert!(
                screen.contains(body_action),
                "{name} body should show `{body_action}`\n{screen}"
            );
            assert!(
                footer.contains(footer_action),
                "{name} footer should show `{footer_action}`\n{screen}"
            );
            assert_eq!(
                footer.matches(footer_action).count(),
                1,
                "{name} footer should show `{footer_action}` once\n{screen}"
            );
            if let Some(forbidden) = forbidden {
                assert!(
                    !footer.contains(forbidden),
                    "{name} footer advertised unrelated `{forbidden}`\n{screen}"
                );
            }
        }
    }

    #[test]
    fn renderer_command_center_all_clear_running_agents_is_state_plus_safe_action() {
        let first = sample_pane("codex");
        let mut second = sample_pane("claude");
        second.id = String::from("%2");
        second.window_id = String::from("@2");
        second.window_name = String::from("builder");
        second.active = false;
        second.pane_index = 1;
        let mut app = app_with_panes(
            vec![first, second],
            vec![
                ("%1", vec!["STATUS=running | NEXT=write tests"]),
                ("%2", vec!["STATUS=running | NEXT=compile release"]),
            ],
        );
        app.show_command_center();

        let lines = render_grid(&app, 110, 18);
        let screen = screen_text(&lines);
        let footer = lines.last().expect("footer should render");

        assert_render_invariants(&lines, 110, 18);
        assert!(screen.contains("Command Center"), "{screen}");
        assert!(screen.contains("All clear: 2 agents working"), "{screen}");
        assert!(
            screen.contains("Action: Enter output demo / agents"),
            "{screen}"
        );
        assert!(
            line_index(&lines, "All clear:") < line_index(&lines, "Action:"),
            "{screen}"
        );
        assert!(!screen.contains("Needs you:"), "{screen}");
        assert!(
            !screen.contains("Working:"),
            "all-clear state should not repeat the same working count:\n{screen}"
        );
        assert!(footer.contains("Enter output"), "{screen}");
        assert!(
            !footer.contains(": send"),
            "all-clear Command Center should not make send look like the next step:\n{screen}"
        );
    }

    #[test]
    fn usability_action_contract_command_center_continue_primary_action_sends_enter() {
        let log_path = std::env::temp_dir().join(format!(
            "muxboard-command-center-continue-{}-{}.log",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock should be after unix epoch")
                .as_nanos()
        ));
        let fake_tmux = fake_tmux_script(
            "command-center-continue",
            &format!(
                "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nexit 0\n",
                log_path.display().to_string().replace('\'', "'\\''")
            ),
        );
        let mut app = app_with_panes(
            vec![sample_pane("codex")],
            vec![("%1", vec!["Press Enter to continue."])],
        );
        use_fake_tmux_for_test(&mut app, fake_tmux);
        app.show_command_center();

        let lines = render_grid(&app, 96, 16);
        let screen = screen_text(&lines);
        let footer = lines.last().expect("footer should render");

        assert_render_invariants(&lines, 96, 16);
        assert!(screen.contains("Command Center"), "{screen}");
        assert!(screen.contains("Action: A continue"), "{screen}");
        assert!(
            footer.contains("A continue"),
            "Command Center footer should expose the visible primary action:\n{screen}"
        );

        tokio::runtime::Runtime::new()
            .expect("runtime should build")
            .block_on(handle_key_press(&mut app, KeyCode::Char('a')))
            .expect("visible Command Center continue action should run");

        assert_eq!(
            app.status_message(),
            "Sent Enter to demo / agents. Watching for update."
        );
        assert!(!app.should_quit());
        let sent = std::fs::read_to_string(&log_path).expect("fake tmux log should be written");
        assert!(
            sent.contains("send-keys -t %1 Enter"),
            "Command Center A continue should send Enter to the waiting pane:\n{sent}"
        );
        for destructive in ["kill-pane", "kill-session", "detach-client"] {
            assert!(
                !sent.contains(destructive),
                "Command Center continue must not dispatch `{destructive}`:\n{sent}"
            );
        }
        let _ = std::fs::remove_file(log_path);

        let after_lines = render_grid(&app, 96, 16);
        let after = screen_text(&after_lines);
        assert_render_invariants(&after_lines, 96, 16);
        assert!(after.contains("Sent Enter to demo"), "{after}");
        assert!(after.contains("Command Center"), "{after}");
        assert!(after.contains("Watching: demo / agents"), "{after}");
        assert!(after.contains("Action: G show in tmux"), "{after}");
        assert!(after.contains("> demo / agents: sent Enter"), "{after}");
        assert!(!after.contains("Queue"), "{after}");
        assert!(!after.contains("Action: : send"), "{after}");
    }

    #[test]
    fn renderer_command_center_post_action_promotes_next_item_without_hiding_watching_item() {
        let runtime = tokio::runtime::Runtime::new().expect("runtime should build");
        let fake_tmux = fake_tmux_script("command-center-render-watching", "#!/bin/sh\nexit 0\n");
        let mut first = sample_pane("codex");
        first.id = String::from("%1");
        first.window_name = String::from("alpha");
        let mut second = sample_pane("claude");
        second.id = String::from("%2");
        second.window_id = String::from("@2");
        second.window_name = String::from("beta");
        second.active = false;
        second.pane_index = 1;
        let mut app = app_with_panes(
            vec![first, second],
            vec![
                ("%1", vec!["Press Enter to continue."]),
                ("%2", vec!["Press Enter to continue."]),
            ],
        );
        use_fake_tmux_for_test(&mut app, fake_tmux);
        app.show_command_center();

        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Char('a')))
            .expect("Command Center continue should send Enter");

        let lines = render_grid(&app, 110, 18);
        let screen = screen_text(&lines);
        let footer = lines.last().expect("footer should render");

        assert_render_invariants(&lines, 110, 18);
        assert!(
            screen.contains("Action: A continue demo / beta"),
            "{screen}"
        );
        assert!(
            screen.contains("> continue demo / beta: needs Enter"),
            "{screen}"
        );
        assert!(screen.contains("Watching"), "{screen}");
        assert!(screen.contains("demo / alpha: sent Enter"), "{screen}");
        assert!(
            screen.contains("Sent Enter to demo / alpha. Next: demo / beta."),
            "{screen}"
        );
        assert!(
            footer.contains("Next: demo / beta"),
            "footer feedback should name the promoted next item:\n{screen}"
        );
        let queue_row = line_index(&lines, "> continue demo / beta");
        let watching_row = line_index(&lines, "demo / alpha: sent Enter");
        assert!(
            queue_row < watching_row,
            "next actionable item should render before watching state:\n{screen}"
        );
    }

    #[test]
    fn usability_action_contract_command_center_answer_primary_action_opens_answer_options() {
        let runtime = tokio::runtime::Runtime::new().expect("runtime should build");
        let fake_tmux = fake_tmux_script("command-center-answer", "#!/bin/sh\nexit 0\n");
        let mut app = app_with_panes(
            vec![sample_pane("claude")],
            vec![("%1", vec!["Allow command? [y/n]"])],
        );
        use_fake_tmux_for_test(&mut app, fake_tmux);
        app.show_command_center();

        let lines = render_grid(&app, 96, 16);
        let screen = screen_text(&lines);
        let footer = lines.last().expect("footer should render");

        assert_render_invariants(&lines, 96, 16);
        assert!(screen.contains("Command Center"), "{screen}");
        assert!(
            screen.contains("Action: . answer demo / agents"),
            "{screen}"
        );
        assert!(screen.contains("> answer demo / agents"), "{screen}");
        assert!(footer.contains(". answer"), "{screen}");
        assert!(
            !footer.contains(". more"),
            "Command Center should not advertise the same key for answer and More:\n{screen}"
        );

        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Char('.')))
            .expect("visible Command Center answer action should open choices");

        let menu = screen_text(&render_grid(&app, 96, 16));
        assert!(menu.contains("More"), "{menu}");
        assert!(menu.contains("Y answer yes"), "{menu}");
        assert!(menu.contains("N answer no"), "{menu}");

        runtime
            .block_on(handle_key_press(&mut app, KeyCode::Char('y')))
            .expect("listed answer key should send yes");

        assert_eq!(
            app.status_message(),
            "Answered yes in demo / agents. Watching for update."
        );
        assert!(!app.should_quit());
    }

    #[test]
    fn usability_action_contract_command_center_off_selection_actions_target_attention_pane() {
        let runtime = tokio::runtime::Runtime::new().expect("runtime should build");
        let log_path = std::env::temp_dir().join(format!(
            "muxboard-command-center-off-selection-{}-{}.log",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock should be after unix epoch")
                .as_nanos()
        ));
        let fake_tmux = fake_tmux_script(
            "command-center-off-selection",
            &format!(
                "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nif [ \"$1\" = \"display-message\" ]; then echo '/dev/ttys999'; exit 0; fi\nexit 0\n",
                log_path.display().to_string().replace('\'', "'\\''")
            ),
        );

        let running = {
            let mut pane = sample_pane("codex");
            pane.id = String::from("%1");
            pane.window_id = String::from("@1");
            pane.window_name = String::from("build");
            pane
        };
        let waiting = {
            let mut pane = sample_pane("claude");
            pane.id = String::from("%2");
            pane.window_id = String::from("@2");
            pane.window_name = String::from("approval");
            pane.pane_index = 1;
            pane.active = false;
            pane
        };

        let mut continue_app = app_with_panes(
            vec![running.clone(), waiting.clone()],
            vec![
                ("%1", vec!["Running cargo test"]),
                ("%2", vec!["Press Enter to continue."]),
            ],
        );
        use_fake_tmux_for_test(&mut continue_app, fake_tmux.clone());
        continue_app.show_command_center();

        let continue_screen = screen_text(&render_grid(&continue_app, 110, 18));
        assert!(continue_screen.contains("Action: A continue demo / approval"));
        assert!(continue_screen.contains("Selected: demo / build"));

        runtime
            .block_on(handle_key_press(&mut continue_app, KeyCode::Char('a')))
            .expect("Command Center continue should target the attention pane");

        assert_eq!(
            continue_app.status_message(),
            "Sent Enter to demo / approval. Watching for update."
        );
        let log = std::fs::read_to_string(&log_path).expect("fake tmux log should be written");
        assert!(log.contains("send-keys -t %2 Enter"), "{log}");
        assert!(!log.contains("send-keys -t %1 Enter"), "{log}");

        let mut error_app = app_with_panes(
            vec![running.clone(), waiting.clone()],
            vec![
                ("%1", vec!["Running cargo test"]),
                ("%2", vec!["error: build failed"]),
            ],
        );
        error_app.show_command_center();

        let error_screen = screen_text(&render_grid(&error_app, 110, 18));
        assert!(error_screen.contains("Action: Enter output demo / approval"));
        assert!(error_screen.contains("Selected: demo / build"));

        runtime
            .block_on(handle_key_press(&mut error_app, KeyCode::Enter))
            .expect("Command Center output should open the attention pane output");

        let output = screen_text(&render_grid(&error_app, 110, 18));
        assert!(error_app.is_output_view_active(), "{output}");
        assert!(output.contains("Output"), "{output}");
        assert!(output.contains("build failed"), "{output}");
        assert!(output.contains("demo/approval"), "{output}");

        let mut reply_app = app_with_panes(
            vec![running.clone(), waiting.clone()],
            vec![
                ("%1", vec!["Running cargo test"]),
                ("%2", vec!["Type your answer to continue."]),
            ],
        );
        use_fake_tmux_for_test(&mut reply_app, fake_tmux.clone());
        reply_app.show_command_center();

        let reply_screen = screen_text(&render_grid(&reply_app, 110, 18));
        assert!(reply_screen.contains("Action: : reply to demo / approval"));
        assert!(reply_screen.contains("Selected: demo / build"));

        runtime
            .block_on(handle_key_press(&mut reply_app, KeyCode::Char(':')))
            .expect("Command Center reply should select the attention pane and open Reply");

        let reply = screen_text(&render_grid(&reply_app, 110, 18));
        assert!(reply_app.is_command_input_active(), "{reply}");
        assert!(reply.contains("Reply"), "{reply}");
        assert!(reply.contains("Reply to: demo / approval"), "{reply}");
        let log = std::fs::read_to_string(&log_path).expect("fake tmux log should be written");
        assert!(!log.contains("select-pane -t %2"), "{log}");
        assert!(!log.contains("select-pane -t %1"), "{log}");

        for ch in "ship it".chars() {
            runtime
                .block_on(handle_key_press(&mut reply_app, KeyCode::Char(ch)))
                .expect("Reply typing should stay inside muxboard");
        }
        runtime
            .block_on(handle_key_press(&mut reply_app, KeyCode::Enter))
            .expect("Reply submit should target the attention pane");

        assert!(!reply_app.is_command_input_active());
        assert_eq!(
            reply_app.status_message(),
            "Sent reply to demo / approval. Watching for update."
        );
        let log = std::fs::read_to_string(&log_path).expect("fake tmux log should be written");
        assert!(log.contains("send-keys -t %2 -l -- ship it"), "{log}");
        assert!(log.contains("send-keys -t %2 Enter"), "{log}");
        assert!(!log.contains("select-pane -t %2"), "{log}");
        assert!(!log.contains("select-pane -t %1"), "{log}");

        let mut answer_app = app_with_panes(
            vec![running, waiting],
            vec![
                ("%1", vec!["Running cargo test"]),
                ("%2", vec!["Allow command? [y/n]"]),
            ],
        );
        use_fake_tmux_for_test(&mut answer_app, fake_tmux);
        answer_app.show_command_center();

        let answer_screen = screen_text(&render_grid(&answer_app, 110, 18));
        assert!(answer_screen.contains("Action: . answer demo / approval"));
        assert!(answer_screen.contains("Selected: demo / build"));

        runtime
            .block_on(handle_key_press(&mut answer_app, KeyCode::Char('.')))
            .expect("Command Center answer should select the attention pane and open More");

        let answer_menu = screen_text(&render_grid(&answer_app, 110, 18));
        assert!(answer_menu.contains("More"), "{answer_menu}");
        assert!(
            answer_menu.contains("Action: Y answer yes"),
            "{answer_menu}"
        );
        assert!(answer_menu.contains("To: demo / approval"), "{answer_menu}");
        let log = std::fs::read_to_string(&log_path).expect("fake tmux log should be written");
        assert!(!log.contains("select-pane -t %2"), "{log}");
        assert!(!log.contains("select-pane -t %1"), "{log}");
        assert!(!log.contains("kill-pane"), "{log}");
        assert!(!log.contains("kill-session"), "{log}");
        assert!(!log.contains("detach-client"), "{log}");
        let _ = std::fs::remove_file(log_path);
    }

    #[test]
    fn usability_action_contract_command_center_output_and_send_actions_open_promised_surfaces() {
        let runtime = tokio::runtime::Runtime::new().expect("runtime should build");
        let press = |app: &mut App, key| {
            runtime
                .block_on(handle_key_press(app, key))
                .expect("visible Command Center primary action should route")
        };
        let assert_action = |app: &App, name: &str, body: &str, footer_action: &str| {
            let lines = render_grid(app, 96, 16);
            let screen = screen_text(&lines);
            let footer = lines.last().expect("footer should render");

            assert_render_invariants(&lines, 96, 16);
            assert!(screen.contains("Command Center"), "{name}\n{screen}");
            assert!(
                screen.contains(body),
                "{name} body should promise `{body}`:\n{screen}"
            );
            assert!(
                footer.contains(footer_action),
                "{name} footer should expose `{footer_action}`:\n{screen}"
            );
        };

        let mut error = app_with_panes(
            vec![sample_pane("codex")],
            vec![("%1", vec!["error: build failed"])],
        );
        error.show_command_center();
        assert_action(
            &error,
            "error Command Center",
            "Action: Enter output",
            "Enter output",
        );

        press(&mut error, KeyCode::Enter);
        let output_lines = render_grid(&error, 96, 16);
        let output = screen_text(&output_lines);
        let output_footer = output_lines.last().expect("footer should render");

        assert_render_invariants(&output_lines, 96, 16);
        assert!(error.is_output_view_active(), "{output}");
        assert!(output.contains("Output"), "{output}");
        assert!(output.contains("build failed"), "{output}");
        assert!(output_footer.contains("Esc back"), "{output}");

        let mut idle = app_with_panes(vec![sample_pane("codex")], vec![("%1", vec!["working"])]);
        idle.show_command_center();
        assert_action(
            &idle,
            "idle Command Center",
            "Action: Enter output",
            "Enter output",
        );

        press(&mut idle, KeyCode::Enter);
        let output_lines = render_grid(&idle, 96, 16);
        let output = screen_text(&output_lines);
        let output_footer = output_lines.last().expect("footer should render");

        assert_render_invariants(&output_lines, 96, 16);
        assert!(idle.is_output_view_active(), "{output}");
        assert!(output.contains("Output"), "{output}");
        assert!(output.contains("working"), "{output}");
        assert!(!output.contains("Send"), "{output}");
        assert!(output_footer.contains("Esc back"), "{output}");

        let mut prompt = app_with_panes(
            vec![sample_pane("codex")],
            vec![("%1", vec!["Type your answer to continue."])],
        );
        prompt.show_command_center();
        assert_action(
            &prompt,
            "prompt Command Center",
            "Action: : reply",
            ": reply",
        );

        press(&mut prompt, KeyCode::Char(':'));
        let reply_lines = render_grid(&prompt, 96, 16);
        let reply = screen_text(&reply_lines);
        let reply_footer = reply_lines.last().expect("footer should render");

        assert_render_invariants(&reply_lines, 96, 16);
        assert!(prompt.is_command_input_active(), "{reply}");
        assert!(reply.contains("Reply"), "{reply}");
        assert!(reply.contains("Reply to: demo / agents"), "{reply}");
        assert!(reply.contains("Text: _"), "{reply}");
        assert!(reply_footer.contains("Enter reply"), "{reply}");
        assert!(reply_footer.contains("Esc cancel"), "{reply}");
    }

    #[test]
    fn usability_action_contract_command_center_empty_refresh_primary_action_runs() {
        let log_path = std::env::temp_dir().join(format!(
            "muxboard-command-center-refresh-{}-{}.log",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock should be after unix epoch")
                .as_nanos()
        ));
        let fake_tmux = fake_tmux_script(
            "command-center-refresh",
            &format!(
                "#!/bin/sh\n\
printf '%s\\n' \"$*\" >> '{}'\n\
if [ \"$1\" = \"-V\" ]; then echo 'tmux fake'; exit 0; fi\n\
if [ \"$1\" = \"list-panes\" ]; then exit 0; fi\n\
if [ \"$1\" = \"-C\" ]; then exit 0; fi\n\
exit 0\n",
                log_path.display().to_string().replace('\'', "'\\''")
            ),
        );
        let mut app = app_with_panes(Vec::new(), vec![]);
        use_fake_tmux_for_test(&mut app, fake_tmux);
        app.show_command_center();

        let lines = render_grid(&app, 96, 16);
        let screen = screen_text(&lines);
        let footer = lines.last().expect("footer should render");

        assert_render_invariants(&lines, 96, 16);
        assert!(screen.contains("Command Center"), "{screen}");
        assert!(
            screen.contains("Action: start tmux panes, then R refresh"),
            "{screen}"
        );
        assert!(!screen.contains("Send:"), "{screen}");
        assert!(!screen.contains("Start:"), "{screen}");
        assert!(
            footer.contains("R refresh"),
            "empty Command Center footer should expose refresh recovery:\n{screen}"
        );

        tokio::runtime::Runtime::new()
            .expect("runtime should build")
            .block_on(handle_key_press(&mut app, KeyCode::Char('r')))
            .expect("visible Command Center refresh action should run");

        assert_eq!(app.status_message(), "Refreshed.");
        assert!(!app.should_quit());
        assert_eq!(app.snapshot().pane_count(), 0);
        let refreshed = screen_text(&render_grid(&app, 96, 16));
        assert!(refreshed.contains("Command Center"), "{refreshed}");
        assert!(refreshed.contains("No panes yet."), "{refreshed}");
        assert!(refreshed.contains("Refreshed."), "{refreshed}");

        let log = std::fs::read_to_string(&log_path).expect("fake tmux log should be written");
        assert!(log.contains("-V"), "{log}");
        assert!(log.contains("list-panes"), "{log}");
        let _ = std::fs::remove_file(log_path);
    }

    #[test]
    fn usability_command_center_no_match_footer_prioritizes_recovery_over_inert_actions() {
        let mut waiting = sample_pane("codex");
        waiting.id = String::from("%1");

        let mut running = sample_pane("claude");
        running.id = String::from("%2");
        running.window_id = String::from("@1");
        running.window_name = String::from("agents-2");
        running.active = false;
        running.pane_index = 1;

        let mut app = app_with_panes(
            vec![waiting, running],
            vec![
                ("%1", vec!["Waiting for approval. Continue?"]),
                ("%2", vec!["thinking..."]),
            ],
        );
        app.show_command_center();
        app.begin_search();
        for ch in "zzz-no-match".chars() {
            app.push_search_char(ch);
        }
        app.finish_search();

        let lines = render_grid(&app, 96, 16);
        let screen = screen_text(&lines);
        let footer = lines.last().expect("footer should render");
        let lower_footer = footer.to_ascii_lowercase();

        assert_render_invariants(&lines, 96, 16);
        assert!(screen.contains("Command Center"), "{screen}");
        assert!(
            screen.contains("Action: backspace show all panes"),
            "{screen}"
        );
        assert!(!screen.contains("Send:"), "{screen}");
        assert!(!screen.contains("Start:"), "{screen}");
        assert!(
            lower_footer.contains("backspace show all"),
            "no-match footer should keep the recovery action visible:\n{screen}"
        );
        assert_eq!(
            lower_footer.matches("backspace show all").count(),
            1,
            "no-match footer should not duplicate the recovery action:\n{screen}"
        );
        for inert in ["J/K move", ": send", "Enter output"] {
            assert!(
                !footer.contains(inert),
                "no-match footer advertised inert `{inert}`:\n{screen}"
            );
        }

        tokio::runtime::Runtime::new()
            .expect("runtime should build")
            .block_on(handle_key_press(&mut app, KeyCode::Backspace))
            .expect("visible recovery action should run");
        let recovered_lines = render_grid(&app, 96, 16);
        let recovered = screen_text(&recovered_lines);
        let recovered_footer = recovered_lines.last().expect("footer should render");

        assert_render_invariants(&recovered_lines, 96, 16);
        assert!(recovered.contains("Command Center"), "{recovered}");
        assert!(!recovered.contains("no matches"), "{recovered}");
        assert!(recovered.contains("Needs you: 1 waiting"), "{recovered}");
        assert!(
            !recovered_footer
                .to_ascii_lowercase()
                .contains("backspace show all"),
            "recovered Command Center footer should stop advertising no-match recovery:\n{recovered}"
        );
    }

    #[test]
    fn usability_command_center_target_action_reads_like_a_sentence() {
        let pane = sample_pane("codex");
        let mut default_app = app_with_panes(vec![pane.clone()], vec![("%1", vec!["working"])]);
        default_app.show_command_center();

        let default_lines = render_grid(&default_app, 96, 16);
        let default_screen = screen_text(&default_lines);

        assert_render_invariants(&default_lines, 96, 16);
        assert!(
            default_screen.contains("All clear: 1 agent working"),
            "{default_screen}"
        );
        assert!(
            default_screen.contains("Action: Enter output demo / agents"),
            "{default_screen}"
        );
        assert!(!default_screen.contains(": send"), "{default_screen}");
        assert!(!default_screen.contains("add panes"), "{default_screen}");

        let mut app = app_with_panes(vec![pane], vec![("%1", vec!["working"])]);
        app.toggle_selected_mark();
        app.show_command_center();

        let lines = render_grid(&app, 96, 16);
        let screen = screen_text(&lines);

        assert_render_invariants(&lines, 96, 16);
        assert!(screen.contains("Command Center"), "{screen}");
        assert!(
            screen.contains("Action: : send to the send list (1 pane)"),
            "{screen}"
        );
        assert!(!screen.contains("send send list"), "{screen}");
        assert!(
            line_index(&lines, "Action:") < line_index(&lines, "Target:"),
            "{screen}"
        );
    }

    #[test]
    fn output_overlay_handles_long_partial_progress_without_smearing_layout() {
        let pane = sample_pane("claude");
        let mut app = app_with_panes(vec![pane], vec![]);
        app.cycle_context_pane();
        set_live_partial_runtime(
            &mut app,
            "%1",
            "tool bash running",
            "Tool Bash running for 300s while compiling a very long dependency graph",
        );

        for &(width, height) in &[(68, 14), (84, 16), (110, 18)] {
            let lines = render_grid(&app, width, height);
            let screen = screen_text(&lines);
            assert_render_invariants(&lines, width, height);
            assert!(screen.contains("Output"), "{width}x{height}\n{screen}");
            assert!(screen.contains("Summary"), "{width}x{height}\n{screen}");
            assert!(
                screen.contains("wait for Bash"),
                "{width}x{height}\n{screen}"
            );
            assert!(
                screen.contains("Tool Bash running"),
                "{width}x{height}\n{screen}"
            );
            assert!(screen.contains("? help"), "{width}x{height}\n{screen}");
        }
    }

    #[test]
    fn dense_fleet_with_near_identical_names_stays_legible() {
        let panes = (0..6)
            .map(|index| {
                let mut pane = sample_pane("node");
                pane.id = format!("%{}", index + 1);
                pane.session_id = String::from("$0");
                pane.session_name = if index < 3 {
                    String::from("alpha-prod")
                } else {
                    String::from("alpha-stage")
                };
                pane.window_id = format!("@{}", index + 1);
                pane.window_name = if index % 2 == 0 {
                    String::from("agent-runner")
                } else {
                    String::from("agent-review")
                };
                pane.pane_index = 0;
                pane.active = index == 0;
                pane
            })
            .collect::<Vec<_>>();
        let runtimes = vec![
            (
                "%1",
                vec!["codex", "STATUS=running | BLOCKER=none | NEXT=write tests"],
            ),
            (
                "%2",
                vec![
                    "codex",
                    "STATUS=waiting | BLOCKER=network access | NEXT=approve request",
                ],
            ),
            (
                "%3",
                vec!["codex", "STATUS=running | BLOCKER=none | NEXT=review diff"],
            ),
            (
                "%4",
                vec!["codex", "STATUS=running | BLOCKER=none | NEXT=ship release"],
            ),
            (
                "%5",
                vec![
                    "codex",
                    "STATUS=error | BLOCKER=command failed | NEXT=show output",
                ],
            ),
            (
                "%6",
                vec!["codex", "STATUS=running | BLOCKER=none | NEXT=sync results"],
            ),
        ];
        let app = app_with_panes(panes, runtimes);

        for &(width, height) in &[(84, 16), (120, 18)] {
            let lines = render_grid(&app, width, height);
            let screen = screen_text(&lines);
            assert_render_invariants(&lines, width, height);
            assert!(screen.contains("alpha"), "{width}x{height}\n{screen}");
            assert!(screen.contains("write tests"), "{width}x{height}\n{screen}");
            assert!(
                screen.contains("approve request") || screen.contains("network access"),
                "{width}x{height}\n{screen}"
            );
            assert!(screen.contains("? help"), "{width}x{height}\n{screen}");
        }
    }

    #[test]
    fn low_level_layout_theme_and_cell_helpers_cover_edge_states() {
        assert_eq!(
            BodyLayoutMode::for_area(83, 10, false, false, false),
            BodyLayoutMode::Stack
        );
        assert_eq!(
            BodyLayoutMode::for_area(99, 14, false, false, false),
            BodyLayoutMode::Stack
        );
        assert_eq!(
            BodyLayoutMode::for_area(109, 22, false, false, false),
            BodyLayoutMode::Stack
        );
        assert_eq!(
            BodyLayoutMode::for_area(110, 22, false, false, false),
            BodyLayoutMode::Stack
        );
        assert_eq!(
            BodyLayoutMode::for_area(112, 22, false, false, false),
            BodyLayoutMode::Stack
        );
        assert_eq!(
            BodyLayoutMode::for_area(120, 22, false, false, false),
            BodyLayoutMode::SplitColumns
        );

        let contrast = Theme::from_preset(ThemePreset::Contrast);
        assert_eq!(contrast.accent, Color::LightCyan);
        assert_eq!(contrast.success, Color::LightGreen);
        assert_eq!(contrast.warning, Color::LightYellow);

        let mono = Theme::from_preset(ThemePreset::Mono);
        assert!(
            mono.selected_row_style()
                .add_modifier
                .contains(Modifier::BOLD)
        );
        assert!(
            mono.selected_row_style()
                .add_modifier
                .contains(Modifier::REVERSED)
        );
        assert!(
            mono.targeted_row_style()
                .add_modifier
                .contains(Modifier::UNDERLINED)
        );
        assert!(
            mono.alert_row_style()
                .add_modifier
                .contains(Modifier::REVERSED)
        );
        assert!(
            mono.confirm_row_style()
                .add_modifier
                .contains(Modifier::BOLD)
        );

        let row = BoardRow {
            selected: true,
            active: true,
            marked: true,
            targeted: true,
            staged: false,
            show_command_in_latest: true,
            attention: String::from("!"),
            status: String::from("running"),
            lifecycle: String::from("working"),
            mission: String::from("ship release after tests"),
            heat: String::from("hot"),
            age: String::from("0s"),
            cpu: String::from("1%"),
            mem: String::from("2m"),
            lane: String::from("Codex"),
            pane: String::from("%1"),
            location: String::from("very-long-location-name#42"),
            command: String::from("codex-agent"),
            title: String::from("ship release after tests"),
        };
        assert_eq!(board_cells(&row, BoardLayoutMode::Full).len(), 5);
        assert_eq!(board_cells(&row, BoardLayoutMode::Standard).len(), 4);
        assert_eq!(board_cells(&row, BoardLayoutMode::Compact).len(), 3);
        assert_eq!(truncate_cell("ab", 2), "ab");
        assert_eq!(truncate_cell("abcdef", 3), "abc");
        assert_eq!(truncate_location_cell("abcdef#longsuffix", 6), "abc...");
        assert_eq!(truncate_location_cell("abcdef#1", 5), "ab...");
        assert_eq!(truncate_panel_line("abcdef", 0), "");
        assert_eq!(truncate_panel_line("abcdef", 3), "abc");
        assert_eq!(key_token(&KeyCode::F(1)), None);
    }

    #[test]
    fn priority_helpers_keep_useful_lines_under_severe_height_pressure() {
        assert_eq!(
            prioritize_context_panel_lines(
                "Other",
                vec![String::from("one"), String::from("two")],
                1
            ),
            vec![String::from("one"), String::from("two")]
        );
        assert_eq!(
            prioritize_context_panel_lines("Details", vec![String::from("one")], 0),
            vec![String::from("one")]
        );

        let selected = prioritize_context_panel_lines(
            "Details",
            vec![
                String::from("demo / agent"),
                String::new(),
                String::from("State: Waiting   Tool: Codex"),
                String::from("Agent report"),
                String::from("Action: : reply"),
                String::from("Seen: 1s"),
                String::from("Output"),
                String::from("old output"),
                String::from("new output"),
                String::from("Updated: 1s ago"),
                String::from("Lane: Codex waiting"),
            ],
            8,
        );
        assert!(selected.contains(&String::from("demo / agent")));
        assert!(selected.contains(&String::from("Agent report")));
        assert!(selected.contains(&String::from("Action: : reply")));
        assert!(selected.contains(&String::from("Output")));
        assert!(selected.contains(&String::from("new output")));

        assert_eq!(
            prioritize_selected_section_items(
                "Agent report",
                vec![
                    String::from("report misc : skipped"),
                    String::from("Status: waiting"),
                    String::from("Blocked: approval"),
                    String::from("Action: continue"),
                    String::from("Seen: 2s"),
                ],
                3,
                0,
            )
            .0,
            vec![
                String::from("Status: waiting"),
                String::from("Blocked: approval"),
                String::from("Action: continue"),
            ]
        );
        assert!(
            prioritize_selected_section_items("Output", vec![String::from("a")], 0, 0)
                .0
                .is_empty()
        );
        assert_eq!(
            prioritize_output_latest_lines(
                vec![
                    String::from("one"),
                    String::from("two"),
                    String::from("three"),
                ],
                2,
            ),
            vec![String::from("two"), String::from("three")]
        );
        assert_eq!(
            prioritize_selected_section_items(
                "Output",
                vec![
                    String::from("one"),
                    String::from("two"),
                    String::from("three"),
                ],
                2,
                99,
            )
            .0,
            vec![String::from("one"), String::from("two")]
        );

        let roomy_details = prioritize_context_panel_lines(
            "Details",
            vec![
                String::from("demo / agent"),
                String::from("State: Running   Tool: Codex"),
                String::from("Agent report"),
                String::from("Status: running"),
                String::from("Blocked: none"),
                String::from("Action: keep working"),
                String::from("Seen: 1s ago"),
                String::from("Output"),
                String::from("output 1"),
                String::from("output 2"),
                String::from("output 3"),
                String::from("output 4"),
                String::from("output 5"),
                String::from("output 6"),
                String::from("output 7"),
                String::from("output 8"),
                String::from("output 9"),
                String::from("Command"),
                String::from("send summary"),
                String::from("Updated: 1s ago"),
                String::from("Lane: Codex running"),
            ],
            18,
        );
        assert_eq!(roomy_details.len(), 18, "{roomy_details:?}");
        assert!(roomy_details.contains(&String::from("Agent report")));
        assert!(roomy_details.contains(&String::from("Status: running")));
        assert!(roomy_details.contains(&String::from("Blocked: none")));
        assert!(roomy_details.contains(&String::from("Output")));
        for index in 1..=9 {
            assert!(
                roomy_details.contains(&format!("output {index}")),
                "{roomy_details:?}"
            );
        }
        assert!(roomy_details.contains(&String::from("Updated: 1s ago")));
        assert!(!roomy_details.contains(&String::from("Lane: Codex running")));
        assert!(!roomy_details.contains(&String::from("Action: keep working")));
        assert!(!roomy_details.contains(&String::from("send summary")));

        let tiny_details = prioritize_context_panel_lines(
            "Details",
            vec![
                String::from("demo / agent"),
                String::from("State: Waiting   Tool: Codex"),
                String::from("Blocked: approval"),
                String::from("Action: continue"),
                String::from("Mission: release"),
                String::from("Output"),
                String::from("new output"),
            ],
            3,
        );
        assert_eq!(
            tiny_details,
            vec![
                String::from("demo / agent"),
                String::from("State: Waiting   Tool: Codex"),
                String::from("Blocked: approval"),
            ]
        );

        let pruned_empty_sections = prioritize_context_panel_lines(
            "Details",
            vec![
                String::from("demo / agent"),
                String::from("State: Running   Tool: Codex"),
                String::from("Agent report"),
                String::from("Updated: 1s ago"),
                String::from("Output"),
                String::from("useful output"),
                String::from("newer output"),
                String::from("Command"),
                String::from("Lane: Codex running"),
                String::from("pane CPU/mem: pid 10 | cpu 1.0% | mem 2.0% | 3s"),
                String::from("Review: send to selected"),
                String::from("Updated: 2s ago"),
            ],
            11,
        );
        assert_eq!(
            pruned_empty_sections,
            vec![
                String::from("demo / agent"),
                String::from("State: Running   Tool: Codex"),
                String::new(),
                String::from("Output"),
                String::from("useful output"),
                String::from("newer output"),
                String::from("Updated: 1s ago"),
                String::from("Lane: Codex running"),
                String::from("pane CPU/mem: pid 10 | cpu 1.0% | mem 2.0% | 3s"),
                String::from("Review: send to selected"),
                String::from("Updated: 2s ago"),
            ]
        );
    }

    #[test]
    fn overlay_priority_helpers_cover_sparse_overflow_and_fallback_paths() {
        assert!(
            prioritize_actions_overlay_lines(
                vec![
                    String::new(),
                    String::new(),
                    String::new(),
                    String::new(),
                    String::new(),
                ],
                4,
            )
            .is_empty()
        );
        let actions = prioritize_actions_overlay_lines(
            vec![
                String::from("ready"),
                String::from("View"),
                String::from("open output"),
                String::from("focus details"),
                String::from("send list"),
                String::from("clear targets"),
                String::from("Pane"),
                String::from("jump"),
                String::from("zoom"),
                String::from("Settings"),
                String::from("theme"),
                String::from("Reports"),
                String::from("summarize"),
            ],
            8,
        );
        assert!(actions.contains(&String::from("open output")));
        assert!(actions.contains(&String::from("jump")));
        assert!(actions.contains(&String::from("clear targets")));

        let empty_view_section = prioritize_actions_overlay_lines(
            vec![
                String::from("ready"),
                String::from("View"),
                String::from("Pane"),
                String::from("J show in tmux"),
                String::from("Z zoom pane"),
                String::from("Reports"),
                String::from("S summarize"),
            ],
            3,
        );
        assert_eq!(
            empty_view_section,
            vec![
                String::from("ready"),
                String::from("J show in tmux"),
                String::from("S summarize"),
            ]
        );

        let pane_items = vec![
            String::from("Y answer yes"),
            String::from("N answer no"),
            String::from("Z zoom pane"),
        ];
        assert!(prioritize_action_section_items("pane", &pane_items, 0).is_empty());
        assert_eq!(
            prioritize_action_section_items("pane", &pane_items, 1),
            vec![String::from("Y answer yes")]
        );
        let waiting_pane_items = vec![
            String::from("I continue waiting panes"),
            String::from("C mute alert"),
            String::from("Space remove from send list"),
            String::from("Z zoom pane"),
            String::from("E send Enter"),
            String::from("B send lane"),
        ];
        assert_eq!(
            prioritize_action_section_items("pane", &waiting_pane_items, 3),
            vec![
                String::from("I continue waiting panes"),
                String::from("B send lane"),
                String::from("Z zoom pane")
            ]
        );
        let view_items = vec![
            String::from("Enter show output"),
            String::from("] command center"),
        ];
        assert_eq!(
            prioritize_action_section_items("view", &view_items, 1),
            vec![String::from("] command center")]
        );
        let send_list_items = vec![
            String::from("L choose fleet"),
            String::from("D delete triage"),
            String::from("X clear send list"),
            String::from("G save fleet"),
        ];
        assert_eq!(
            prioritize_action_section_items("send list", &send_list_items, 2),
            vec![
                String::from("X clear send list"),
                String::from("G save fleet")
            ]
        );
        let stale_fleet_items = vec![
            String::from("L choose fleet"),
            String::from("D delete stale triage"),
        ];
        assert_eq!(
            prioritize_action_section_items("send list", &stale_fleet_items, 2),
            stale_fleet_items
        );

        let overview = prioritize_overview_overlay_lines(
            vec![
                String::from("fleet live"),
                String::from("Queue (3)"),
                String::from("continue codex"),
                String::from("answer claude"),
                String::from("output opencode"),
                String::from("+ 5 more need you: continue"),
                String::from("Lanes"),
                String::from("codex running"),
                String::from("claude idle"),
                String::from("shell idle"),
                String::from("node running"),
                String::from("bash idle"),
                String::from("> opencode waiting"),
            ],
            9,
        );
        assert!(overview.contains(&String::from("Queue (3)")));
        assert!(overview.contains(&String::from("continue codex")));
        assert!(overview.contains(&String::from("+ 5 more need you: continue")));
        assert!(!overview.contains(&String::from("output opencode")));
        assert!(overview.contains(&String::from("Lanes")));
        assert!(overview.contains(&String::from("> opencode waiting")));

        let output_without_summary = prioritize_output_overlay_lines(
            vec![
                String::from("demo / pane"),
                String::from("Latest"),
                String::from("one"),
                String::from("two"),
                String::from("three"),
                String::from("four"),
            ],
            5,
        );
        assert_eq!(output_without_summary.last(), Some(&String::from("four")));

        let output_with_summary = prioritize_output_overlay_lines(
            vec![
                String::from("demo / pane"),
                String::from("Summary"),
                String::from("waiting approval"),
                String::from("Latest"),
                String::from("old"),
                String::from("new"),
            ],
            6,
        );
        assert!(output_with_summary.contains(&String::from("Summary")));
        assert!(output_with_summary.contains(&String::from("waiting approval")));

        let tiny_output = prioritize_output_overlay_lines(
            vec![
                String::from("demo / pane"),
                String::from("Running | 0s ago"),
                String::from("Summary"),
                String::new(),
                String::from("waiting approval"),
                String::from("Latest"),
                String::from("old"),
                String::new(),
                String::from("newest tail"),
            ],
            3,
        );
        assert_eq!(
            tiny_output,
            vec![
                String::from("demo / pane"),
                String::from("old"),
                String::from("newest tail"),
            ]
        );

        assert_eq!(
            prioritize_output_overlay_lines(
                vec![
                    String::from("demo / pane"),
                    String::from("Latest"),
                    String::from("old"),
                    String::from("newest"),
                ],
                1,
            ),
            vec![String::from("newest")]
        );

        let roomy_output = prioritize_output_overlay_lines(
            vec![
                String::from("demo / pane"),
                String::from("State: Waiting"),
                String::from("Summary"),
                String::from("waiting approval"),
                String::from("Latest"),
                String::from("old"),
                String::from("middle"),
                String::from("new"),
                String::from("newest"),
                String::from("tail"),
                String::from("done"),
                String::from("extra"),
                String::from("overflow"),
            ],
            11,
        );
        assert_eq!(
            roomy_output,
            vec![
                String::from("demo / pane"),
                String::from("State: Waiting"),
                String::new(),
                String::from("Latest"),
                String::from("new"),
                String::from("newest"),
                String::from("tail"),
                String::from("done"),
                String::from("extra"),
                String::from("overflow"),
                String::from("waiting approval"),
            ]
        );
    }

    #[test]
    fn fleet_picker_priority_keeps_the_selected_saved_fleet_visible() {
        let lines = (0..10)
            .map(|index| {
                if index == 4 {
                    format!("> fleet-{index}")
                } else {
                    format!("  fleet-{index}")
                }
            })
            .collect::<Vec<_>>();
        let visible = prioritize_fleet_picker_lines(lines, 5);
        assert_eq!(
            visible,
            vec![
                String::from("  fleet-2"),
                String::from("  fleet-3"),
                String::from("> fleet-4"),
                String::from("  fleet-5"),
                String::from("  fleet-6"),
            ]
        );

        let bottom_selected = (0..10)
            .map(|index| {
                if index == 9 {
                    format!("> fleet-{index}")
                } else {
                    format!("  fleet-{index}")
                }
            })
            .collect::<Vec<_>>();
        let visible = prioritize_fleet_picker_lines(bottom_selected, 5);
        assert_eq!(
            visible,
            vec![
                String::from("  fleet-5"),
                String::from("  fleet-6"),
                String::from("  fleet-7"),
                String::from("  fleet-8"),
                String::from("> fleet-9"),
            ]
        );

        let unmarked = (0..6)
            .map(|index| format!("  fleet-{index}"))
            .collect::<Vec<_>>();
        assert_eq!(
            prioritize_fleet_picker_lines(unmarked, 3),
            vec![
                String::from("  fleet-0"),
                String::from("  fleet-1"),
                String::from("  fleet-2"),
            ]
        );

        let zero_height = vec![String::from("> fleet-0"), String::from("  fleet-1")];
        assert_eq!(
            prioritize_fleet_picker_lines(zero_height.clone(), 0),
            zero_height
        );
    }

    #[test]
    fn send_priority_helpers_cover_confirm_preview_vars_and_overflow() {
        let send = prioritize_send_overlay_lines(
            vec![
                String::from("send to 3 panes"),
                String::from("send list fleet review"),
                String::from("fleet saved"),
                String::from("Action: confirm"),
                String::from("start type command"),
                String::from("vars {pane} {session}"),
                String::from("alerts"),
                String::from("waiting approval"),
                String::from("review"),
                String::from("send to 3 panes"),
                String::from("send cargo test"),
                String::from("  cargo test -q"),
                String::from("  ... 2 more panes"),
                String::from("preview"),
                String::from("one"),
                String::from("two"),
                String::from("... more"),
                String::from("Reports"),
                String::from("Action: test"),
            ],
            10,
        );
        assert!(send.contains(&String::from("send to 3 panes")));
        assert!(send.contains(&String::from("Targets")));
        assert!(send.contains(&String::from("send cargo test")));

        let hidden_send_list = prioritize_send_overlay_lines(
            vec![
                String::from("send list (2 panes, 1 hidden)"),
                String::from("send list 2 panes"),
                String::from("1 pane hidden by current view"),
                String::from("Action: : send list"),
                String::from("preview"),
                String::from("demo / alpha : echo %1"),
                String::from("demo / beta (hidden) : echo %2"),
            ],
            6,
        );
        assert!(hidden_send_list.contains(&String::from("send list (2 panes, 1 hidden)")));
        assert!(hidden_send_list.contains(&String::from("1 pane hidden by current view")));
        assert!(!hidden_send_list.contains(&String::from("send list 2 panes")));

        let fallback = prioritize_send_overlay_lines(
            vec![
                String::from("custom command"),
                String::from("another"),
                String::from("third"),
            ],
            1,
        );
        assert_eq!(fallback, vec![String::from("custom command")]);

        let vars = prioritize_send_overlay_lines(
            vec![
                String::from("send to selected"),
                String::from("vars {pane} {session}"),
                String::from("Recent"),
                String::from("old command"),
            ],
            6,
        );
        assert!(vars.contains(&String::from("vars {pane} {session}")));

        let useful_vars = prioritize_send_overlay_lines(
            vec![
                String::from("send to selected"),
                String::from("Action: : review"),
                String::from("vars {pane} {session} {window}"),
                String::from("Recent"),
                String::from("old command"),
                String::from("Macros"),
                String::from("1 cargo test"),
            ],
            5,
        );
        assert_eq!(
            useful_vars,
            vec![
                String::from("send to selected"),
                String::from("Action: : review"),
                String::new(),
                String::from("vars {pane} {session} {window}"),
            ]
        );
        assert!(!useful_vars.contains(&String::from("old command")));
        assert!(!send.contains(&String::from("vars {pane} {session}")));

        let cramped = prioritize_send_overlay_lines(
            vec![
                String::from("send to 8 panes"),
                String::from("send list (8 panes, 5 hidden)"),
                String::from("5 panes hidden by current view"),
                String::from("fleet overnight"),
                String::from("Action: confirm"),
                String::from("start codex"),
                String::from("alerts"),
                String::from("waiting approval"),
                String::from("preview"),
                String::from("demo / alpha : cargo test"),
            ],
            3,
        );
        assert_eq!(
            cramped,
            vec![
                String::from("send to 8 panes"),
                String::from("send list (8 panes, 5 hidden)"),
                String::from("5 panes hidden by current view"),
            ]
        );

        assert!(prioritize_send_section_items("review", vec![String::from("x")], 0).is_empty());
        assert_eq!(
            prioritize_confirm_send_items(
                vec![
                    String::from("send to 2 panes"),
                    String::from("send hello"),
                    String::from("  hello"),
                    String::from("  ... 1 more"),
                    String::from("send hello"),
                ],
                4,
            ),
            vec![
                String::from("send to 2 panes"),
                String::from("send hello"),
                String::from("  hello"),
                String::from("  ... 1 more"),
            ]
        );
        assert_eq!(
            prioritize_confirm_send_items(
                vec![
                    String::from("To: the send list (2 panes, 1 hidden)"),
                    String::from("1 pane hidden by current view"),
                    String::from("Text: hello"),
                    String::from("  demo / alpha hello"),
                    String::from("  demo / beta (hidden) hello"),
                    String::from("  ... 1 more"),
                ],
                4,
            ),
            vec![
                String::from("To: the send list (2 panes, 1 hidden)"),
                String::from("1 pane hidden by current view"),
                String::from("Text: hello"),
                String::from("  demo / beta (hidden) hello"),
            ]
        );
        assert_eq!(
            prioritize_preview_send_items(
                vec![
                    String::from("demo / alpha : hello"),
                    String::from("demo / zeta (hidden) : hello"),
                    String::from("... : 2 more panes"),
                ],
                2,
            ),
            vec![
                String::from("demo / zeta (hidden) : hello"),
                String::from("... : 2 more panes"),
            ]
        );
        assert_eq!(
            prioritize_preview_send_items(
                vec![String::from("... only overflow"), String::from("line")],
                1,
            ),
            vec![String::from("line")]
        );
        assert_eq!(
            prioritize_preview_send_items(vec![String::from("... only overflow")], 2),
            vec![String::from("... only overflow")]
        );
        assert_eq!(
            prioritize_generic_send_items(
                vec![
                    String::from("one"),
                    String::from("two"),
                    String::from("... more"),
                ],
                2,
            ),
            vec![String::from("one"), String::from("... more")]
        );
    }
}
