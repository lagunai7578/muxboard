use std::{
    fs,
    path::{Path, PathBuf},
};

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

const THEME_PRESET_NAMES: &[&str] = &[
    "Calm",
    "Contrast",
    "Mono",
    "TerminalNative",
    "CatppuccinLatte",
    "CatppuccinMocha",
    "TokyoNight",
    "GruvboxDark",
    "GruvboxLight",
    "Nord",
    "RosePine",
];

const RETIRED_PRODUCT_COPY_TERMS: &[&str] = &[
    "Ready to send",
    "staged send",
    "staged command",
    "Enter inspect",
    "Space target",
    "target panes",
    "target set",
    "target group",
    "Target group",
    "send target",
    "target hidden by current view",
    "targets hidden by current view",
    "No target panes remain",
    "Start target disappeared",
    "Selected pane",
    "inspects here",
    "Smart action",
    "smart action",
    "smart move",
    "G tmux",
    "G jump",
    "G open",
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
    "pane(s)",
    "group(s)",
    "alert(s)",
    "send to send list",
    "send to list",
    "next nothing waiting",
    "shown next move",
    "Secondary view switched",
    "Cleared pane search",
    "Closed pane search input",
    "Details focused",
    "Navigator selected",
    "No panes in view.",
    "Clear scope or wait for tmux.",
    "add panes",
    "add/remove list",
    "command selected/list",
    "fanout off",
    "fanout lane",
    "lane fanout",
    "Lane fanout",
    "enabling lane fanout",
    "clear search or scope",
    "Clear search or scope",
    "Scoped the board",
    "backspace all panes",
    "Backspace returns to all panes",
    "backspace returns to all panes",
    "alert debounce",
    "Alert debounce",
    "alert rule",
    "Alert policy",
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
    "`t` sort",
    "`f` filter",
];

const RETIRED_PUBLIC_COPY_EXTRA_TERMS: &[&str] = &[
    "Nothing needs attention",
    "1 attention",
    "2 attention",
    "3 attention",
    "4 attention",
    "action none waiting",
    "attention 0 wait",
    "attention 1 wait",
    " total |",
    "vars {session}",
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
    "saved commands",
    "Saved group",
    "No saved groups",
    "group triage",
    "zoom, groups",
    "load next fleet",
    "choose action",
    " / %",
    "send yes",
    "send no",
    "open tmux panes",
    "tmux-open",
    "Esc quits immediately",
    "Advanced controls now live behind",
    "The current problem",
];

#[test]
fn core_stays_isolated_from_app_tui_and_tmux() -> TestResult<()> {
    for file in rust_files_under("src/core")?
        .into_iter()
        .chain(optional_file("src/core.rs"))
    {
        let source = fs::read_to_string(&file)?;
        assert_not_contains(&file, &source, "crate::app")?;
        assert_not_contains(&file, &source, "crate::tui")?;
        assert_not_contains(&file, &source, "use crate::tmux")?;
        assert_not_contains(&file, &source, "crate::tmux")?;
        assert_not_contains(&file, &source, "tmux::")?;
    }

    Ok(())
}

#[test]
fn tui_talks_to_app_not_core_or_tmux() -> TestResult<()> {
    for file in rust_files_under("src/tui")?
        .into_iter()
        .chain(optional_file("src/tui.rs"))
    {
        let source = fs::read_to_string(&file)?;
        assert_not_contains(&file, &source, "crate::core")?;
        assert_not_contains(&file, &source, "use crate::tmux")?;
        assert_not_contains(&file, &source, "crate::tmux")?;
        assert_not_contains(&file, &source, "tmux::")?;
    }

    Ok(())
}

#[test]
fn app_stays_free_of_terminal_widget_dependencies() -> TestResult<()> {
    for file in rust_files_under("src/app")?
        .into_iter()
        .chain(optional_file("src/app.rs"))
    {
        let source = fs::read_to_string(&file)?;
        assert_not_contains(&file, &source, "ratatui")?;
        assert_not_contains(&file, &source, "crossterm")?;
    }

    Ok(())
}

#[test]
fn public_copy_and_fixtures_avoid_retired_product_terms() -> TestResult<()> {
    let mut files = [
        "AGENTS.md",
        "CHANGELOG.md",
        "README.md",
        "CONTRIBUTING.md",
        "docs/agent-view-audit.md",
        "docs/muxboard-demo.svg",
        "docs/provider-drift.md",
        "docs/testing-matrix.md",
        "docs/ui-reboot.md",
        "config.example.json",
        "tests/fixtures/app/panels.json",
        "tests/fixtures/app/view_models.json",
    ]
    .into_iter()
    .map(|relative| manifest_path().join(relative))
    .collect::<Vec<_>>();
    files.extend(golden_screen_files()?);

    for file in files {
        let source = fs::read_to_string(&file)?;
        for term in RETIRED_PRODUCT_COPY_TERMS
            .iter()
            .chain(RETIRED_PUBLIC_COPY_EXTRA_TERMS)
        {
            assert_not_contains(&file, &source, term)?;
        }
    }

    Ok(())
}

#[test]
fn public_demo_svg_matches_current_product_language() -> TestResult<()> {
    let path = manifest_path().join("docs/muxboard-demo.svg");
    let source = fs::read_to_string(&path)?;
    let required = [
        "Fleet |",
        "Details",
        "Command Center",
        "Browse",
        "Output",
        "Needs you:",
        "Action:",
        "Latest",
        "? help",
    ];
    let banned = [
        "STATUS=",
        "BLOCKER=",
        "NEXT=",
        "Details pane",
        "Live tail",
        "Session tree",
        "Board |",
        "control mode",
        "pane %",
        "next action",
        "review, alert",
    ];

    for phrase in required {
        assert_contains(&path, &source, phrase)?;
    }
    assert_not_contains_any(&path, &source, &banned)?;
    assert_not_contains(&path, &source, "<tspan")?;

    Ok(())
}

#[test]
fn production_user_copy_avoids_retired_product_terms() -> TestResult<()> {
    let files = [
        "src/app.rs",
        "src/app/presentation.rs",
        "src/app/targets.rs",
        "src/tui.rs",
    ];
    for relative in files {
        let path = manifest_path().join(relative);
        let mut source = fs::read_to_string(&path)?;
        if let Some((production, _tests)) = source.split_once("\n#[cfg(test)]\nmod tests") {
            source = production.to_owned();
        }
        for term in RETIRED_PRODUCT_COPY_TERMS {
            assert_not_contains(&path, &source, term)?;
        }
    }

    Ok(())
}

#[test]
fn user_visible_target_counts_stay_self_explanatory() -> TestResult<()> {
    let mut files = [
        "src/app.rs",
        "src/app/presentation.rs",
        "src/app/targets.rs",
        "src/app/tests.rs",
        "src/tui.rs",
        "tests/live_e2e.rs",
        "tests/fixtures/app/panels.json",
        "tests/fixtures/app/view_models.json",
    ]
    .into_iter()
    .map(|relative| manifest_path().join(relative))
    .collect::<Vec<_>>();
    files.extend(golden_screen_files()?);

    for file in files {
        let source = fs::read_to_string(&file)?;
        assert_no_terse_target_count_copy(&file, &source)?;
    }

    Ok(())
}

#[test]
fn target_count_guard_catches_escaped_copy_shapes() -> TestResult<()> {
    let rust_file = Path::new("src/tui.rs");
    let visible_file = Path::new("tests/fixtures/tui/golden/example.txt");

    for line in [
        r#"String::from("send list 1")"#,
        r#"String::from("review 2")"#,
        r#"String::from("send to fleet triage (3)")"#,
        r#"String::from("lane Codex (4)")"#,
        r#"String::from("send to Codex lane (5)")"#,
    ] {
        assert!(
            assert_no_terse_target_count_copy(rust_file, line).is_err(),
            "guard should reject escaped terse copy: {line}"
        );
    }

    for line in [
        r#"assert!(app.board_title(8).contains("send list 1 pane"));"#,
        r#"String::from("send list (1 pane)")"#,
        r#"String::from("review 2 panes")"#,
        r#"String::from("send to fleet triage (3 panes)")"#,
        r#"String::from("send to Codex lane (4 panes)")"#,
        r#"let lanes = self.visible_agent_lanes(5);"#,
        "Review send to 3 panes.",
        "send to fleet triage (3 panes)",
    ] {
        assert_no_terse_target_count_copy(
            if line.contains("String::from") || line.contains("assert!") || line.contains("let ") {
                rust_file
            } else {
                visible_file
            },
            line,
        )?;
    }

    Ok(())
}

#[test]
fn public_readme_stays_scan_first_and_current() -> TestResult<()> {
    let path = manifest_path().join("README.md");
    let source = fs::read_to_string(&path)?;
    let required = [
        "What you see first:",
        "Fleet: every pane",
        "Details: the selected pane",
        "Output: a deeper, scrollable view",
        "Footer: the next safe keys",
        "Power stays one layer down",
        "review-before-send broadcasts",
        "Browse for sessions and windows",
        "Command Center for fleet triage",
        "On first run, muxboard opens a small theme picker",
        "muxboard --theme-picker",
        "muxboard --theme system",
        "muxboard --print-config-example >",
        "ui_settings.theme.preset` supports `Calm`, `Contrast`, `Mono`, `TerminalNative`, `CatppuccinLatte`, `CatppuccinMocha`, `TokyoNight`, `GruvboxDark`, `GruvboxLight`, `Nord`, and `RosePine`",
        "Kebab-case aliases like `light`, `dark`, `system`, `system-colors`, `catppuccin-mocha`, `tokyo-night`, `gruvbox`, `rose-pine`, `terminal`, `ansi`, and `no-color` also work.",
        "Named themes are semantic mappings into muxboard's slots",
        "Muxboard does not read terminal config files directly",
        "The older `theme_preset` field still works",
        "ui_settings.theme.overrides",
    ];
    let banned = [
        "It now includes:",
        "The generated config starts like this:",
        "\"notification_settings\": {",
        "\"keybindings\": {",
        "dense board view",
        "live tail panel",
        "one-shot fleet summary polling",
        "parsed agent self-reports",
        "board and detail panels",
        "run the shown action",
        "Secondary direct keys",
    ];

    for phrase in required {
        assert_contains(&path, &source, phrase)?;
    }
    assert_not_contains_any(&path, &source, &banned)?;

    Ok(())
}

#[test]
fn public_project_surface_stays_release_ready() -> TestResult<()> {
    let required_files = [
        ".github/ISSUE_TEMPLATE/bug_report.yml",
        ".github/ISSUE_TEMPLATE/feature_request.yml",
        ".github/ISSUE_TEMPLATE/config.yml",
        ".github/pull_request_template.md",
        "SECURITY.md",
        "CODE_OF_CONDUCT.md",
        "docs/index.html",
        "docs/social-preview.svg",
        "docs/social-preview.png",
        "docs/demo.md",
        "scripts/demo-session",
        "docs/goals/demo-polish.md",
    ];

    for relative in required_files {
        let path = manifest_path().join(relative);
        if !path.exists() {
            return Err(format!("public project surface is missing {}", path.display()).into());
        }
    }

    let readme_path = manifest_path().join("README.md");
    let readme = fs::read_to_string(&readme_path)?;
    for phrase in [
        "A tmux command center for AI agents",
        "Why muxboard?",
        "Try it",
        "Download release",
        "Want a safe first look?",
        "isolated tmux socket",
        "just demo-start",
        "just demo-attach",
        "just demo-stop",
        "generic fake panes",
        "does not attach to your live tmux server",
        "Recording, GIF, MP4, and screenshot instructions live in [`docs/demo.md`](docs/demo.md)",
        "no account, no cloud service, and no repo or worktree inspection",
        "Default key: `prefix` + `M`",
        "popup command center",
        "dock: real tmux sidebar pane",
        "drawer: temporary right-side overlay",
    ] {
        assert_contains(&readme_path, &readme, phrase)?;
    }

    let pages_path = manifest_path().join("docs/index.html");
    let pages = fs::read_to_string(&pages_path)?;
    for phrase in [
        "A command center for tmux agent fleets.",
        "og:url",
        "https://raw.githubusercontent.com/aanari/muxboard/main/docs/social-preview.png",
        "summary_large_image",
        "cargo install --git https://github.com/aanari/muxboard --locked",
        "muxboard-demo.svg",
        "social-preview.png",
        "Private demo",
        "Safe first look",
        "private tmux socket",
        "No account, cloud service, or repo scan",
        "docs/demo.md",
        "docs/tmux-plugin.md",
        "prefix",
    ] {
        assert_contains(&pages_path, &pages, phrase)?;
    }
    assert_not_contains(&pages_path, &pages, "https://aanari.github.io/muxboard")?;

    for relative in [
        "README.md",
        "docs/demo.md",
        "docs/index.html",
        "Cargo.toml",
        ".github/ISSUE_TEMPLATE/config.yml",
        ".github/ISSUE_TEMPLATE/bug_report.yml",
        ".github/ISSUE_TEMPLATE/feature_request.yml",
    ] {
        let path = manifest_path().join(relative);
        let source = fs::read_to_string(&path)?;
        assert_not_contains(&path, &source, "https://aanari.github.io/muxboard")?;
        assert_not_contains(&path, &source, "anari.io/muxboard")?;
    }

    let social_path = manifest_path().join("docs/social-preview.svg");
    let social = fs::read_to_string(&social_path)?;
    for phrase in [
        "a tmux command center for AI agent fleets",
        "1 needs you, 2 working",
        "A tmux command center for AI agent fleets.",
    ] {
        assert_contains(&social_path, &social, phrase)?;
    }
    assert_not_contains(&social_path, &social, "3 need you")?;

    assert_png_dimensions(&manifest_path().join("docs/social-preview.png"), 1200, 630)?;

    let security_path = manifest_path().join("SECURITY.md");
    let security = fs::read_to_string(&security_path)?;
    assert_contains(
        &security_path,
        &security,
        "https://github.com/aanari/muxboard/security/advisories/new",
    )?;

    let demo_path = manifest_path().join("docs/demo.md");
    let demo = fs::read_to_string(&demo_path)?;
    for phrase in [
        "Private demo guide",
        "private tmux server",
        "never touches or records your",
        "live tmux server",
        "First look",
        "just demo-start",
        "just demo-attach",
        "just demo-record",
        "just demo-stop",
        "just demo-smoke",
        "Export media",
        "just demo-assets",
        "just public-assets",
        "just demo-mp4",
        "brew install imagemagick",
        "brew install asciinema agg ffmpeg",
        "Do not commit raw recordings",
    ] {
        assert_contains(&demo_path, &demo, phrase)?;
    }
    assert_not_contains(&demo_path, &demo, "VHS")?;

    let script_path = manifest_path().join("scripts/demo-session");
    let script = fs::read_to_string(&script_path)?;
    for phrase in [
        "MUXBOARD_DEMO_SOCKET",
        "muxboard-demo",
        "target/demo/muxboard.cast",
        "target/demo/muxboard.mp4",
        "target/demo/assets",
        "docs/social-preview.png",
        "asciinema rec",
        "agg \"$cast\" \"$gif\"",
        "ffmpeg -y",
        "FONTCONFIG_FILE=\"$fonts\"",
        "PNG32:$dest",
    ] {
        assert_contains(&script_path, &script, phrase)?;
    }

    let justfile_path = manifest_path().join("justfile");
    let justfile = fs::read_to_string(&justfile_path)?;
    for phrase in [
        "demo-gif:",
        "demo-mp4:",
        "demo-assets:",
        "public-assets:",
        "demo-check:",
    ] {
        assert_contains(&justfile_path, &justfile, phrase)?;
    }

    Ok(())
}

#[test]
fn readme_shortcuts_match_generated_default_keybindings() -> TestResult<()> {
    let path = manifest_path().join("README.md");
    let source = fs::read_to_string(&path)?;
    let defaults: serde_json::Value =
        serde_json::from_str(&muxboard::config::default_keybindings_json()?)?;

    let label = |name| primary_keybinding_label(&defaults, name);
    let quick_start = [
        format!(
            "`{}` / `{}` to move",
            label("move_down")?,
            label("move_up")?
        ),
        format!("`{}` to show the selected pane output", label("focus")?),
        format!("`{}` to show the selected pane in tmux", label("jump")?),
        format!("`{}` to add or remove panes", label("mark")?),
        format!("`{}` to type a command", label("command")?),
        format!("`{}` to continue a waiting pane", label("smart_action")?),
        format!(
            "`{}` to ask panes for one-line summaries",
            label("summaries")?
        ),
        format!("`{}` to clear the send list", label("clear_marks")?),
        format!("`{}` to search", label("search")?),
        format!("`{}` to quit", label("quit")?),
    ];
    let direct = [
        format!(
            "`{}` switches focus between Fleet and Details when both surfaces are active",
            label("panel_focus")?
        ),
        format!("`{}` refreshes", label("refresh")?),
        format!(
            "on the Send surface, `{}` repeats the most recent command",
            label("repeat_last")?
        ),
        format!(
            "on the Send surface, `{}` then a macro key pins",
            label("macro_assign")?
        ),
    ];
    let more = [
        format!("Press `{}`", label("actions")?),
        format!("after pressing `{}`", label("actions")?),
        String::from("Labels adapt to the current surface"),
        format!(
            "`{}` shows output, returns to details, or opens a Browse window",
            label("focus")?
        ),
        format!(
            "`{}` browses tmux sessions and windows",
            label("action_view_browse")?
        ),
        format!(
            "`{}` shows Command Center",
            label("action_view_command_center")?
        ),
        format!("`{}` mute selected alert", label("action_ack_selected")?),
        format!(
            "`{}` unmute selected alert",
            label("action_ack_clear_selected")?
        ),
        format!("`{}` mute all alerts", label("action_ack_all")?),
        format!("`{}` unmute all alerts", label("action_ack_clear_all")?),
        format!("`{}` continue waiting panes", label("action_enter_queue")?),
        format!("`{}` send `Enter`", label("action_send_enter")?),
        format!("`{}` send `y`", label("action_send_yes")?),
        format!("`{}` send `n`", label("action_send_no")?),
        format!("`{}` zoom", label("action_zoom")?),
        format!("`{}` start a new agent", label("action_launch_agent")?),
        format!("`{}` change sort order", label("action_sort")?),
        format!("`{}` change visible panes", label("action_filter")?),
        format!("`{}` save a fleet", label("action_group_save")?),
        format!("`{}` choose a saved fleet", label("action_group_load")?),
        format!(
            "`{}` delete the selected saved fleet",
            label("action_group_delete")?
        ),
        format!(
            "`{}` send to the selected lane",
            label("action_lane_target")?
        ),
        format!("`{}` toggle pane CPU/memory", label("action_metrics")?),
        format!(
            "`{}` toggle desktop notifications",
            label("action_desktop_notifications")?
        ),
        format!("`{}` toggle terminal bell", label("action_bell")?),
        format!(
            "`{}` cycle alert repeat delay",
            label("action_alert_debounce")?
        ),
        format!("`{}` cycle alert types", label("action_alert_policy")?),
    ];

    for phrase in quick_start.into_iter().chain(direct).chain(more) {
        assert_contains(&path, &source, &phrase)?;
    }

    Ok(())
}

#[test]
fn tui_colors_stay_behind_semantic_theme_slots() -> TestResult<()> {
    let path = manifest_path().join("src/tui.rs");
    let source = fs::read_to_string(&path)?;
    let production = source
        .split("#[cfg(test)]\nmod tests")
        .next()
        .expect("tui production code should exist");
    let theme_boundary = production
        .find("enum BoardLayoutMode")
        .expect("theme boundary should precede layout code");

    for slot in [
        "text",
        "muted",
        "accent",
        "success",
        "warning",
        "danger",
        "surface",
        "border",
        "selected_fg",
        "selected_bg",
    ] {
        assert_contains(&path, production, slot)?;
    }

    for (line_index, line) in production.lines().enumerate() {
        if line.contains("Color::") {
            let line_offset = production
                .lines()
                .take(line_index)
                .map(|line| line.len() + 1)
                .sum::<usize>();
            if line_offset > theme_boundary {
                return Err(format!(
                    "{}:{} uses a raw Color outside the Theme boundary: {}",
                    path.display(),
                    line_index + 1,
                    line.trim()
                )
                .into());
            }
        }
    }

    Ok(())
}

#[test]
fn theme_inventory_stays_consistent_across_code_docs_and_config_example() -> TestResult<()> {
    let app_path = manifest_path().join("src/app.rs");
    let app_source = fs::read_to_string(&app_path)?;
    let enum_body = app_source
        .split("pub(crate) enum ThemePreset {")
        .nth(1)
        .and_then(|tail| tail.split("\n}").next())
        .ok_or("ThemePreset enum should be readable")?;

    let enum_variants = enum_body
        .lines()
        .map(str::trim)
        .map(|line| line.trim_end_matches(','))
        .filter(|line| THEME_PRESET_NAMES.contains(line))
        .collect::<Vec<_>>();
    assert_eq!(
        enum_variants.len(),
        THEME_PRESET_NAMES.len(),
        "ThemePreset should contain exactly the release-grade inventory"
    );

    let tui_path = manifest_path().join("src/tui.rs");
    let tui_source = fs::read_to_string(&tui_path)?;
    assert_contains(
        &tui_path,
        &tui_source,
        "const ALL_THEME_PRESETS: [ThemePreset; 11]",
    )?;

    let readme_path = manifest_path().join("README.md");
    let readme = fs::read_to_string(&readme_path)?;
    let audit_path = manifest_path().join("docs/theme-audit.md");
    let audit = fs::read_to_string(&audit_path)?;
    let config_path = manifest_path().join("config.example.json");
    let config = fs::read_to_string(&config_path)?;

    for name in THEME_PRESET_NAMES {
        assert_contains(&app_path, &app_source, name)?;
        assert_contains(&tui_path, &tui_source, &format!("ThemePreset::{name}"))?;
        assert_contains(&readme_path, &readme, name)?;
        assert_contains(&audit_path, &audit, name)?;
    }

    for phrase in [
        "terminal",
        "ansi",
        "no-color",
        "catppuccin",
        "gruvbox",
        "rose-pine",
    ] {
        assert_contains(&readme_path, &readme, phrase)?;
        assert_contains(&audit_path, &audit, phrase)?;
    }

    assert_contains(&config_path, &config, r#""preset": "TerminalNative""#)?;
    Ok(())
}

#[test]
fn theme_audit_stays_aligned_with_small_semantic_design() -> TestResult<()> {
    let path = manifest_path().join("docs/theme-audit.md");
    let source = fs::read_to_string(&path)?;

    for phrase in [
        "Muxboard should feel native in a terminal without becoming a theme engine.",
        "ratatui-themes",
        "ratatui-themekit",
        "`tui-theme`: no current crate with that exact name appeared",
        "tui-theme-builder",
        "GitUI",
        "crates-tui",
        "Helix",
        "Yazi",
        "Yazi flavors",
        "Zellij",
        "Lazygit",
        "Starship",
        "Bat",
        "Delta",
        "Catppuccin ports",
        "Tokyo Night ports",
        "Gruvbox",
        "Nord",
        "Rosé Pine",
        "OpenCode",
        "NO_COLOR: environment convention",
        "Comparison matrix",
        "Semantic slots beat widget-level color settings.",
        "Terminal-native themes matter.",
        "Theme names should be forgiving.",
        "Previewability matters, but tests are muxboard's preview.",
        "Full external theme files.",
        "Auto-reading terminal or dotfile configs.",
        "Permanent runtime theme surface.",
        "Pulling a dependency just for palette constants.",
        "Syntax-theme concepts.",
        "Deferred until evidence",
        "Catppuccin Frappe/Macchiato",
        "Light/dark auto mode.",
        "`text`, `muted`, `accent`, `success`, `warning`, `danger`, `surface`, `border`, `selected_fg`, `selected_bg`",
        "11 presets: `Calm`, `Contrast`, `Mono`, `TerminalNative`, `CatppuccinLatte`, `CatppuccinMocha`, `TokyoNight`, `GruvboxDark`, `GruvboxLight`, `Nord`, `RosePine`",
        "Named truecolor presets are semantic mappings into muxboard slots, not full upstream theme ports.",
        "OpenCode exposes this as `system`",
        "label it System Colors",
        "`light`, `dark`, `system`, `system colors`, `terminal`, `ansi`, `no-color`, `catppuccin`, `tokyo night`, `gruvbox`, `rose-pine`, `rosé pine`",
        "`#RGB`, and `#RRGGBB`",
        "First run opens a small picker with System Colors highlighted",
        "Added first-run and explicit `--theme-picker` onboarding",
        "cargo test onboarding",
        "Locked exact truecolor palette tokens for named presets in `cargo test theme`.",
        "prevents raw `Color::` usage from spreading outside the theme boundary",
    ] {
        assert_contains(&path, &source, phrase)?;
    }

    Ok(())
}

#[test]
fn theme_fast_filter_stays_coverage_oriented() -> TestResult<()> {
    let tui_path = manifest_path().join("src/tui.rs");
    let tui_source = fs::read_to_string(&tui_path)?;
    let config_path = manifest_path().join("src/config.rs");
    let config_source = fs::read_to_string(&config_path)?;

    for phrase in [
        "fn theme_no_color_and_dumb_profiles_keep_shape_cues_without_color_dependency",
        "fn calm_theme_keeps_body_text_native_to_survive_light_and_dark_terminals",
        "fn terminal_native_theme_uses_ansi_colors_to_follow_the_terminal_palette",
        "fn default_theme_is_terminal_native_and_does_not_paint_broad_backgrounds",
        "fn default_theme_renderer_keeps_terminal_native_cells",
        "fn theme_named_truecolor_presets_use_documented_palette_tokens",
        "fn usability_theme_presets_keep_selection_alerts_and_targets_distinguishable",
        "fn usability_theme_truecolor_presets_keep_selection_contrast",
        "fn usability_theme_presets_render_core_states_at_cell_level",
        "fn usability_theme_scrollbars_use_accent_and_surface_slots",
        "fn usability_theme_onboarding_picker_is_readable_minimal_and_keyboard_obvious",
        "fn usability_action_contract_theme_onboarding_keys_match_the_footer",
    ] {
        assert_contains(&tui_path, &tui_source, phrase)?;
    }

    for phrase in [
        "fn theme_presets_accept_builtin_palette_names",
        "fn theme_colors_parse_and_serialize_common_terminal_forms",
        "fn load_ui_settings_rejects_bad_theme_colors_with_actionable_copy",
        "fn load_ui_settings_rejects_bad_theme_presets_with_actionable_copy",
        "fn theme_onboarding_detects_missing_theme_without_overwriting_existing_config",
        "fn theme_onboarding_stays_off_for_existing_theme_shapes",
    ] {
        assert_contains(&config_path, &config_source, phrase)?;
    }

    Ok(())
}

#[test]
fn ui_reboot_notes_stay_aligned_with_current_shell_language() -> TestResult<()> {
    let path = manifest_path().join("docs/ui-reboot.md");
    let source = fs::read_to_string(&path)?;
    let required = [
        "Fleet plus Details",
        "Fleet owns the left side.",
        "Details owns the right side.",
        "Default Fleet columns",
        "Details should answer five questions",
        "message may supplement the keymap",
        "J/K moves Fleet selection by default.",
        "Details or Output is focused and scrollable",
    ];
    let banned = [
        "main board",
        "Main board",
        "board rows",
        "Default board",
        "The board owns",
        "detail panel",
        "Detail panel",
        "stack board",
        "status messages can temporarily replace hints",
        "Uses four short lines max.",
    ];

    for phrase in required {
        assert_contains(&path, &source, phrase)?;
    }
    assert_not_contains_any(&path, &source, &banned)?;

    Ok(())
}

#[test]
fn usability_agents_principles_stay_loaded_as_product_guardrails() -> TestResult<()> {
    let agents = fs::read_to_string(manifest_path().join("AGENTS.md"))?;
    let required = [
        "Don't make me think.",
        "Omit, then omit again.",
        "Users scan, they don't read.",
        "Visual hierarchy is everything.",
        "Eliminate noise.",
        "Navigation as Wayfinding",
        "Action Contract QA",
        "TUI Renderer Verification",
        "Performance is Usability",
        "Escaped-Bug Proactive Loop",
        "Coverage X-Ray",
    ];

    for phrase in required {
        assert_contains(manifest_path().join("AGENTS.md").as_path(), &agents, phrase)?;
    }

    Ok(())
}

#[test]
fn product_scope_keeps_v1_tmux_first_without_vcs_dependency() -> TestResult<()> {
    let readme_path = manifest_path().join("README.md");
    let readme = fs::read_to_string(&readme_path)?;
    for phrase in [
        "V1 is intentionally tmux-first and agent-control-first",
        "does not inspect repos, branches, or worktrees",
        "VCS context belongs in V2 as an optional project layer",
    ] {
        assert_contains(&readme_path, &readme, phrase)?;
    }

    let cargo_path = manifest_path().join("Cargo.toml");
    let cargo = fs::read_to_string(&cargo_path)?;
    assert_not_contains_any(&cargo_path, &cargo, &["git2", "gix", "libgit2", "hg"])?;

    let audit_path = manifest_path().join("docs/agent-view-audit.md");
    let audit = fs::read_to_string(&audit_path)?;
    for phrase in [
        "copy the product shape, not the Claude-specific machinery",
        "Do Not Copy For V1",
        "VCS, PR, branch, worktree, or review status",
        "Muxboard's advantage is different",
        "cross-agent and tmux-native",
    ] {
        assert_contains(&audit_path, &audit, phrase)?;
    }

    let opensessions_audit_path = manifest_path().join("docs/opensessions-gap-audit.md");
    let opensessions_audit = fs::read_to_string(&opensessions_audit_path)?;
    for phrase in [
        "Native local source hints for Codex and Claude Code",
        "Explicit tmux bridge state always wins over native hints",
        "Claude parsing uses safe title/task-summary metadata",
        "VCS, PR, branch, dirty-tree, worktree, or review status",
        "OpenCode SQLite ingestion",
        "calm tmux command center for local or SSH agent panes",
    ] {
        assert_contains(&opensessions_audit_path, &opensessions_audit, phrase)?;
    }

    Ok(())
}

#[test]
fn persistence_save_results_stay_must_use_and_visible() -> TestResult<()> {
    let contracts = [
        (
            "src/app.rs",
            vec![
                "#[must_use]\n    fn save_persistent_state",
                "#[must_use]\n    fn save_command_state",
                "#[must_use]\n    fn save_notification_settings",
            ],
        ),
        (
            "src/app/targets.rs",
            vec![
                "#[must_use]\n    pub(super) fn upsert_target_group",
                "#[must_use]\n    pub(super) fn save_target_groups",
            ],
        ),
    ];

    for (relative, phrases) in contracts {
        let path = manifest_path().join(relative);
        let source = fs::read_to_string(&path)?;
        for phrase in phrases {
            assert_contains(&path, &source, phrase)?;
        }
    }

    let bare_ignores = [
        "self.save_persistent_state();",
        "self.save_command_state();",
        "self.save_notification_settings();",
        "self.save_target_groups();",
    ];

    for file in rust_files_under("src")? {
        let source = fs::read_to_string(&file)?;
        for (index, line) in source.lines().enumerate() {
            let trimmed = line.trim();
            if bare_ignores.contains(&trimmed) {
                return Err(format!(
                    "{}:{} must handle save failure visibility instead of discarding `{trimmed}`",
                    file.display(),
                    index + 1
                )
                .into());
            }
        }
    }

    Ok(())
}

#[test]
fn release_gate_stays_comprehensive_and_v1_identified() -> TestResult<()> {
    let cargo_path = manifest_path().join("Cargo.toml");
    let cargo = fs::read_to_string(&cargo_path)?;
    for phrase in [
        "version = \"1.0.0\"",
        "description = \"A tmux command center for AI agents, panes, and long-running terminal work.\"",
        "license = \"Apache-2.0\"",
        "readme = \"README.md\"",
        "repository = \"https://github.com/aanari/muxboard\"",
        "homepage = \"https://github.com/aanari/muxboard\"",
        "documentation = \"https://github.com/aanari/muxboard/tree/main/docs\"",
        "# V1 ships GitHub-first. Remove this only when crates.io publishing is intentional.",
        "publish = false",
        "\"docs/agent-loop-notes.md\"",
        "\"docs/codex-autopass-prompt.md\"",
    ] {
        assert_contains(&cargo_path, &cargo, phrase)?;
    }

    let justfile_path = manifest_path().join("justfile");
    let justfile = fs::read_to_string(&justfile_path)?;
    for phrase in [
        "release-check: ci-full ux coverage-full-gate package-check dogfood",
        "ci: fmt-check lint guards contracts test perf-smoke",
        "ci-full: fmt-check lint guards contracts test perf-smoke test-live",
        "cargo test -- --test-threads=1",
        "test-live:",
        "cargo test --test live_e2e -- --ignored --nocapture --test-threads=1",
        "--fail-under-lines 95",
        "--fail-under-regions 95",
        "--fail-under-functions 95",
        "cargo build --release --locked",
        "target/release/muxboard --version",
        "cargo package --allow-dirty --locked",
        "github-preflight:",
        "gh repo view \"$expected\"",
        "visibility\" = \"PUBLIC\"",
        "homepageUrl",
        "hasDiscussionsEnabled",
        "repositoryTopics",
        "homepage must stay canonical until Pages is verified",
    ] {
        assert_contains(&justfile_path, &justfile, phrase)?;
    }

    let readme_path = manifest_path().join("README.md");
    let readme = fs::read_to_string(&readme_path)?;
    assert_contains(&readme_path, &readme, "just release-check")?;
    assert_contains(
        &readme_path,
        &readme,
        "cargo install --git https://github.com/aanari/muxboard --locked",
    )?;
    assert_contains(&readme_path, &readme, "docs/release.md")?;
    assert_contains(&readme_path, &readme, "Licensed under Apache-2.0")?;

    let release_doc_path = manifest_path().join("docs/release.md");
    let release_doc = fs::read_to_string(&release_doc_path)?;
    for phrase in [
        "Muxboard V1 ships GitHub-first.",
        "just public-assets",
        "just release-check",
        "cargo package --locked --list",
        "public docs, demo SVGs, social-preview PNG",
        "just github-preflight",
        "the GitHub repo exists, the repo is public",
        "Keep the repository homepage pointed at GitHub unless the public Pages route has",
        "owner-level custom",
        "gh repo create aanari/muxboard --public --source . --remote origin --push",
        "cargo install --git https://github.com/aanari/muxboard --locked",
        "git tag -a v1.0.0 -m \"muxboard 1.0.0\"",
        "`publish = false` is intentional for V1.",
    ] {
        assert_contains(&release_doc_path, &release_doc, phrase)?;
    }

    let release_workflow_path = manifest_path().join(".github/workflows/release.yml");
    let release_workflow = fs::read_to_string(&release_workflow_path)?;
    for phrase in [
        "tags:",
        "- \"v*\"",
        "cargo build --release --locked",
        "target/release/muxboard --version",
        "shasum -a 256",
        "actions/upload-artifact@v4",
        "actions/download-artifact@v5",
        "gh release create",
        "--notes-file CHANGELOG.md",
    ] {
        assert_contains(&release_workflow_path, &release_workflow, phrase)?;
    }

    Ok(())
}

#[test]
fn ux_recipe_runs_copy_architecture_and_renderer_guardrails() -> TestResult<()> {
    let justfile_path = manifest_path().join("justfile");
    let justfile = fs::read_to_string(&justfile_path)?;
    let ux_recipe = recipe_body(&justfile, "ux")
        .ok_or_else(|| format!("{} must define `ux`", justfile_path.display()))?;

    for phrase in [
        "just guards",
        "just ux-actions",
        "cargo test --lib usability_ -- --nocapture",
        "just tui-golden",
        "just perf-smoke",
    ] {
        assert_contains(&justfile_path, ux_recipe, phrase)?;
    }

    let tui_golden = recipe_body(&justfile, "tui-golden")
        .ok_or_else(|| format!("{} must define `tui-golden`", justfile_path.display()))?;
    assert_contains(
        &justfile_path,
        tui_golden,
        "cargo test --lib exact_grid_matches -- --nocapture",
    )?;

    let tui_golden_bless = recipe_body(&justfile, "tui-golden-bless")
        .ok_or_else(|| format!("{} must define `tui-golden-bless`", justfile_path.display()))?;
    assert_contains(
        &justfile_path,
        tui_golden_bless,
        "MUXBOARD_BLESS_GOLDEN=1 cargo test --lib exact_grid_matches -- --nocapture",
    )?;

    Ok(())
}

#[test]
fn autonomous_loop_runs_the_active_goal_gates() -> TestResult<()> {
    let justfile_path = manifest_path().join("justfile");
    let justfile = fs::read_to_string(&justfile_path)?;
    let autoloop = recipe_body(&justfile, "codex-autoloop")
        .ok_or_else(|| format!("{} must define `codex-autoloop`", justfile_path.display()))?;

    for phrase in [
        "just codex-autopass",
        "Refusing codex-autoloop on a dirty tree",
        "just ux",
        "just ci",
        "just perf-live",
        "codex-autoloop paused with a reviewable diff",
        "Review, test, and commit before running another pass.",
        "git status --short",
    ] {
        assert_contains(&justfile_path, autoloop, phrase)?;
    }

    let prompt_path = manifest_path().join("docs/codex-autopass-prompt.md");
    let prompt = fs::read_to_string(&prompt_path)?;
    for phrase in [
        "Follow AGENTS.md strictly",
        "Pick exactly one high-value bounded pass",
        "press the advertised keys in tests",
        "just coverage-missing",
        "just ux",
        "just ci",
        "just perf-live",
        "docs/agent-loop-notes.md",
        "Do not commit",
    ] {
        assert_contains(&prompt_path, &prompt, phrase)?;
    }

    Ok(())
}

#[test]
fn saved_codex_goals_are_mobile_friendly_and_guarded() -> TestResult<()> {
    let justfile_path = manifest_path().join("justfile");
    let justfile = fs::read_to_string(&justfile_path)?;

    for recipe in [
        "goal-list",
        "goal-show",
        "goal-buffer",
        "goal-send",
        "goal-run",
        "goal-check",
    ] {
        recipe_body(&justfile, recipe)
            .ok_or_else(|| format!("{} must define `{recipe}`", justfile_path.display()))?;
    }

    for phrase in [
        "ci: fmt-check lint guards contracts test perf-smoke tmux-plugin-check goal-check demo-check",
        "ci-full: fmt-check lint guards contracts test perf-smoke test-live tmux-plugin-check goal-check demo-check",
    ] {
        assert_contains(&justfile_path, &justfile, phrase)?;
    }

    let goal_send = recipe_body(&justfile, "goal-send")
        .ok_or_else(|| format!("{} must define `goal-send`", justfile_path.display()))?;
    assert_contains(&justfile_path, goal_send, "scripts/codex-goal-send")?;

    let goal_run = recipe_body(&justfile, "goal-run")
        .ok_or_else(|| format!("{} must define `goal-run`", justfile_path.display()))?;
    for phrase in [
        "codex exec",
        "--cd \"$PWD\"",
        "--sandbox danger-full-access",
        "-c approval_policy=\\\"never\\\"",
        "-c model_reasoning_effort=\\\"xhigh\\\"",
        "< \"$goal\"",
        "git diff --check",
    ] {
        assert_contains(&justfile_path, goal_run, phrase)?;
    }

    let script_path = manifest_path().join("scripts/codex-goal-send");
    let script = fs::read_to_string(&script_path)?;
    for phrase in [
        "Expected exactly one Codex pane",
        "tmux list-panes -a",
        "#{pane_current_command}",
        "$2 == \"codex\"",
        "tmux load-buffer \"$goal\"",
        "tmux send-keys -t \"$target\" \"/goal\" C-m",
        "tmux paste-buffer -t \"$target\"",
    ] {
        assert_contains(&script_path, &script, phrase)?;
    }

    let readme_path = manifest_path().join("docs/goals/README.md");
    let readme = fs::read_to_string(&readme_path)?;
    for phrase in [
        "Saved goals keep mobile SSH work short",
        "just goal-list",
        "just goal-run agent-view",
        "just goal-run demo-polish",
        "longer GitHub presentation loop",
        "just goal-send agent-view",
        "exactly one Codex tmux pane",
    ] {
        assert_contains(&readme_path, &readme, phrase)?;
    }

    let goal_path = manifest_path().join("docs/goals/agent-view.md");
    let goal = fs::read_to_string(&goal_path)?;
    for phrase in [
        "Audit Claude Code Agent View",
        "attention-first grouping",
        "minimal peek/reply",
        "Do not add VCS",
        "Keep muxboard cross-agent and tmux-native",
    ] {
        assert_contains(&goal_path, &goal, phrase)?;
    }

    let demo_goal_path = manifest_path().join("docs/goals/demo-polish.md");
    let demo_goal = fs::read_to_string(&demo_goal_path)?;
    for phrase in [
        "GitHub documentation, demo media, and launch presentation",
        "Continue sanding recursively",
        "docs/social-preview.png",
        "GIF, MP4, cast, and PNG export paths",
        "just public-assets",
        "never expose real pane data",
        "change owner-level Pages or DNS settings",
        "just demo-smoke",
    ] {
        assert_contains(&demo_goal_path, &demo_goal, phrase)?;
    }

    Ok(())
}

#[test]
fn tmux_plugin_entrypoint_stays_tpm_compatible() -> TestResult<()> {
    let entry_path = manifest_path().join("muxboard.tmux");
    let entry = fs::read_to_string(&entry_path)?;
    for phrase in [
        "#!/usr/bin/env bash",
        "extras/tmux/scripts/muxboard-open",
        "extras/tmux/scripts/muxboard-mark-seen",
        "extras/tmux/scripts/muxboard-status",
        "extras/tmux/scripts/muxboard-session-dots",
        "muxboard_status",
        "muxboard_session_dots",
        "tmux bind-key",
        "@muxboard-key",
        "@muxboard-bind",
        "@muxboard-drawer-key",
        "--toggle-peek",
        "@muxboard-mark-seen-on-focus",
        "register_hook_once",
        "pane-focus-in",
        "after-select-pane",
        "after-select-window",
    ] {
        assert_contains(&entry_path, &entry, phrase)?;
    }

    let mark_seen_path = manifest_path().join("extras/tmux/scripts/muxboard-mark-seen");
    let mark_seen = fs::read_to_string(&mark_seen_path)?;
    for phrase in [
        "#!/usr/bin/env bash",
        "muxboard-mark-seen [--pane <pane_id>]",
        "MUXBOARD_AGENT_PANE_${fragment}_STATE",
        "TMUX_AGENT_PANE_${pane_id}_STATE",
        "review_state_should_clear",
        "tmux set-environment -g \"$muxboard_unseen_key\" \"0\"",
        "tmux set-environment -g \"$legacy_unseen_key\" \"0\"",
        "refresh-client -S",
    ] {
        assert_contains(&mark_seen_path, &mark_seen, phrase)?;
    }

    let helper_path = manifest_path().join("extras/tmux/scripts/muxboard-open");
    let helper = fs::read_to_string(&helper_path)?;
    for phrase in [
        "#!/usr/bin/env bash",
        "parse_args",
        "--preset",
        "--toggle-peek",
        "@muxboard_peek_",
        "display-popup -C",
        "MUXBOARD_TMUX_PEEK_TOGGLE=1",
        "MUXBOARD_TMUX_PEEK_PREFIX2=",
        "sh -lc",
        "@muxboard-open-preset",
        "@muxboard-open-mode",
        "@muxboard-popup-placement",
        "@muxboard-popup-width",
        "@muxboard-popup-height",
        "@muxboard-close-after-jump",
        "@muxboard-command",
        "@muxboard-window-name",
        "@muxboard-reuse-window",
        "@muxboard-split-percent",
        "@muxboard-split-direction",
        "@muxboard-dock-width",
        "@muxboard-dock-percent",
        "@muxboard-dock-side",
        "display-popup",
        "-x \"$popup_x\" -y \"$popup_y\" -w \"$popup_width\" -h \"$popup_height\"",
        "drawer)",
        "dock)",
        "existing_dock_pane",
        "@muxboard_dock",
        "kill-pane -t",
        "supports_full_size_split",
        "mode=\"dock\"",
        "split-window -P -F \"#{pane_id}\"",
        "-f -h",
        "dock_size_flag=\"-l\"",
        "full-window dock requires tmux split-window -f",
        "popup_x=\"R\"",
        "popup_y=\"0\"",
        "MUXBOARD_CLOSE_AFTER_JUMP=%s",
        "new-window",
        "split-window",
        "#{pane_current_path}",
        "@muxboard-start-directory",
        "@muxboard-extra-args",
        "center, drawer, top, bottom, left, right, dock, window, or split",
        "muxboard command not found",
    ] {
        assert_contains(&helper_path, &helper, phrase)?;
    }

    let smoke_path = manifest_path().join("extras/tmux/scripts/muxboard-plugin-smoke");
    let smoke = fs::read_to_string(&smoke_path)?;
    for phrase in [
        "MUXBOARD_FAKE_OPEN_PRESET=drawer run_helper",
        "display-popup -d $START_DIR -x R -y 0 -w 45% -h 100% -E",
        "MUXBOARD_CLOSE_AFTER_JUMP=1",
        "MUXBOARD_FAKE_OPEN_PRESET=dock run_helper --preset drawer",
        "MUXBOARD_FAKE_OPEN_PRESET=dock run_helper --toggle-peek",
        "set-option -gq @muxboard_peek_12345 1",
        "MUXBOARD_TMUX_PEEK_PREFIX=C-b",
        "MUXBOARD_TMUX_PEEK_PREFIX2=",
        "MUXBOARD_FAKE_PREFIX2=C-a run_helper --toggle-peek",
        "MUXBOARD_TMUX_PEEK_KEY=P",
        "MUXBOARD_FAKE_PEEK_OPEN=1 run_helper --toggle-peek",
        "display-popup -C",
        "MUXBOARD_FAKE_OPEN_PRESET=dock run_helper",
        "split-window -P -F #{pane_id} -f -h -b -l 52 -c $START_DIR",
        "set-option -p -t %99 @muxboard_dock 1",
        "MUXBOARD_FAKE_DOCK_SIDE=right",
        "split-window -P -F #{pane_id} -f -h -l 35% -c $START_DIR",
        "MUXBOARD_FAKE_NO_FULL_SPLIT=1",
        "full-window dock requires tmux split-window -f; opening a raw split instead",
        "MUXBOARD_FAKE_DOCK_PANE=%9",
        "kill-pane -t %9",
        "MUXBOARD_FAKE_WINDOW_WIDTH=240",
        "MUXBOARD_FAKE_DOCK_WIDTH=58",
        "assert_log_not_contains \"display-popup\"",
        "display-popup -d $START_DIR -x C -y C -w 90% -h 85% -E",
        "MUXBOARD_CLOSE_AFTER_JUMP=0",
        "MUXBOARD_FAKE_OPEN_MODE=window run_helper",
        "new-window -n muxboard -c $START_DIR",
        "MUXBOARD_FAKE_OPEN_MODE='split' run_helper",
        "split-window -h -l 45% -c $START_DIR",
        "MUXBOARD_FAKE_DRAWER_KEY=P run_entrypoint",
        "bind-key P run-shell -b",
        "--toggle-peek",
        "MUXBOARD_FAKE_MARK_SEEN_ON_FOCUS=off run_entrypoint",
        "set-hook -ag pane-focus-in run-shell -b",
        "muxboard-mark-seen",
        "MUXBOARD_FAKE_STATUS_RIGHT='#{muxboard_session_dots} #{muxboard_status}' run_entrypoint",
        "MUXBOARD_FAKE_AGENT_STATE=approval run_status_helper demo",
        "assert_output_equals \"mux ! codex\"",
        "MUXBOARD_FAKE_AGENT_STATE=complete MUXBOARD_FAKE_AGENT_UNSEEN=seen run_status_helper demo",
        "assert_output_equals \"mux done codex\"",
        "MUXBOARD_FAKE_AGENT_AGENT=\"Claude Code\"",
        "assert_output_equals \"mux ! claude-code\"",
        "MUXBOARD_FAKE_AGENT2_STATE=done",
        "assert_output_equals \"mux !2\"",
        "MUXBOARD_FAKE_DOTS_ATTENTION='A'",
        "MUXBOARD_FAKE_DOTS_ATTENTION_COLOR='yellow'",
        "MUXBOARD_FAKE_USE_LEGACY_AGENT=1 MUXBOARD_FAKE_AGENT_STATE=done MUXBOARD_FAKE_AGENT_UNSEEN=unseen run_status_helper demo",
        "MUXBOARD_FAKE_AGENT_STATE=tool_running",
        "assert_output_equals \".*\"",
        "muxboard-agent-state",
        "MUXBOARD_AGENT_PANE__1_STATE waiting",
        "MUXBOARD_AGENT_PANE__1_THREAD_ID turn-123",
        "MUXBOARD_AGENT_PANE__1_UNSEEN 1",
        "run_mark_seen --pane \"%1\"",
        "TMUX_AGENT_PANE_%1_UNSEEN 0",
        "run_codex_notify permission-request",
    ] {
        assert_contains(&smoke_path, &smoke, phrase)?;
    }

    let docs_path = manifest_path().join("docs/tmux-plugin.md");
    let docs = fs::read_to_string(&docs_path)?;
    for phrase in [
        "set -g @plugin 'aanari/muxboard'",
        "Press `prefix` + `M`",
        "@muxboard-open-preset 'center'",
        "@muxboard-open-preset 'drawer'",
        "@muxboard-open-preset 'dock'",
        "@muxboard-open-preset 'window'",
        "@muxboard-open-preset 'split'",
        "Choose the shape by task",
        "peek drawer",
        "@muxboard-drawer-key 'P'",
        "The peek drawer opens on the right",
        "It floats over panes and never pushes or",
        "Dock opens muxboard as a real full-height tmux sidebar",
        "`prefix` + `M` again to close the dock",
        "`prefix` + `P` toggles the peek",
        "The peek drawer floats and never changes the tmux layout",
        "The sidebar toggles with `prefix` + `M`",
        "The peek drawer toggles with `prefix` +",
        "selected pane",
        "@muxboard-dock-side 'left'",
        "@muxboard-dock-width '58'",
        "pushes the current",
        "@muxboard-popup-placement 'center'",
        "@muxboard-close-after-jump 'off'",
        "@muxboard-mark-seen-on-focus 'on'",
        "@muxboard-open-mode 'popup'",
        "@muxboard-drawer-bind 'on'",
        "Ambient agent attention",
        "muxboard-agent-state",
        "muxboard-codex-notify",
        "--thread-name",
        "--progress",
        "--log",
        "unseen",
        "#{muxboard_status}",
        "mux ! codex",
        "mux + claude",
        "#{muxboard_session_dots}",
        "@muxboard-session-dots-attention-color 'yellow'",
        "Muxboard does not recolor panes or window titles by default",
        "focusing that pane directly in tmux does the same",
        "Waiting states keep showing until you answer them",
        "A separate `muxboard-tmux` repo would only be worth it later",
    ] {
        assert_contains(&docs_path, &docs, phrase)?;
    }

    let readme_path = manifest_path().join("README.md");
    let readme = fs::read_to_string(&readme_path)?;
    assert_contains(&readme_path, &readme, "docs/tmux-plugin.md")?;
    assert_contains(&readme_path, &readme, "set -g @plugin 'aanari/muxboard'")?;

    let justfile_path = manifest_path().join("justfile");
    let justfile = fs::read_to_string(&justfile_path)?;
    for phrase in [
        "tmux-plugin-check:",
        "bash -n muxboard.tmux",
        "bash -n extras/tmux/scripts/muxboard-open",
        "bash -n extras/tmux/scripts/muxboard-mark-seen",
        "bash -n extras/tmux/scripts/muxboard-agent-state",
        "bash -n extras/tmux/scripts/muxboard-codex-notify",
        "bash -n extras/tmux/scripts/muxboard-status",
        "bash -n extras/tmux/scripts/muxboard-session-dots",
        "bash -n extras/tmux/scripts/muxboard-plugin-smoke",
        "test -x extras/tmux/scripts/muxboard-mark-seen",
        "extras/tmux/scripts/muxboard-plugin-smoke",
        "tmux-plugin-live:",
        "tmux_plugin_dock_opens_full_height_sidebar_not_quadrant_split",
        "tmux_plugin_dock",
        "tmux_plugin_drawer_preserves_layout_while_default_is_dock",
        "tmux_plugin_drawer_binding_targets_drawer_while_default_is_dock",
        "tmux_plugin_status_widgets_render_agent_names_and_custom_dots_live",
        "tmux_plugin_focus_marks_terminal_review_seen_live",
        "tmux_plugin_peek_toggle_closes_live_muxboard_popup_without_layout_change",
        "tmux_plugin_peek_toggle_honors_custom_tmux_prefix",
        "tmux_plugin_peek_toggle_honors_tmux_prefix2",
        "tmux_plugin_drawer_close_after_jump_env_defaults_on",
        "tmux_plugin_dock_close_after_jump_closes_live_muxboard_pane",
        "ci: fmt-check lint guards contracts test perf-smoke tmux-plugin-check",
        "ci-full: fmt-check lint guards contracts test perf-smoke test-live tmux-plugin-check",
    ] {
        assert_contains(&justfile_path, &justfile, phrase)?;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        for path in [&entry_path, &helper_path, &mark_seen_path, &smoke_path] {
            let mode = fs::metadata(path)?.permissions().mode();
            if mode & 0o111 == 0 {
                return Err(format!("{} must be executable for TPM", path.display()).into());
            }
        }
    }

    Ok(())
}

#[test]
fn perf_smoke_covers_input_loop_renderer_and_large_fleets() -> TestResult<()> {
    let justfile_path = manifest_path().join("justfile");
    let justfile = fs::read_to_string(&justfile_path)?;
    let perf = recipe_body(&justfile, "perf")
        .ok_or_else(|| format!("{} must define `perf`", justfile_path.display()))?;
    let perf_smoke = recipe_body(&justfile, "perf-smoke")
        .ok_or_else(|| format!("{} must define `perf-smoke`", justfile_path.display()))?;
    let perf_live = recipe_body(&justfile, "perf-live")
        .ok_or_else(|| format!("{} must define `perf-live`", justfile_path.display()))?;

    assert_contains(&justfile_path, perf, "perf-smoke")?;

    for phrase in [
        "large_fleet_presentation_perf_smoke",
        "input_loop_stays_below_human_lag_threshold",
        "navigation_key_burst_stays_in_memory_and_below_human_lag_threshold",
        "output_scroll_key_burst_stays_in_memory_and_below_human_lag_threshold",
        "renderer_navigation_perf_smoke_stays_interactive",
    ] {
        assert_contains(&justfile_path, perf_smoke, phrase)?;
    }
    assert_contains(
        &justfile_path,
        perf_live,
        "large_fleet_navigation_holds_up_with_twenty_panes",
    )?;

    let live_e2e_path = manifest_path().join("tests/live_e2e.rs");
    let live_e2e = fs::read_to_string(&live_e2e_path)?;
    for phrase in [
        "const FAST_POLL_INTERVAL: Duration = Duration::from_millis(25);",
        "const RESPONSIVE_NAVIGATION_TIMEOUT: Duration = Duration::from_secs(2);",
        "const RESPONSIVE_STATE_UPDATE_TIMEOUT: Duration = Duration::from_secs(3);",
        "wait_for_text_with_poll",
        "large fleet navigation should finish inside",
        "live status update should replace stale text inside",
        "wait_for_live_status_summary_with_poll(",
        "selected_fleet_block_contains(",
    ] {
        assert_contains(&live_e2e_path, &live_e2e, phrase)?;
    }

    let agents_path = manifest_path().join("AGENTS.md");
    let agents = fs::read_to_string(&agents_path)?;
    for phrase in [
        "Movement, focus changes, search typing, and opening Output must feel instant.",
        "Simple navigation must stay in-memory.",
        "just perf-smoke",
        "just perf-live",
        "just ci",
        "just release-check",
    ] {
        assert_contains(&agents_path, &agents, phrase)?;
    }

    Ok(())
}

#[test]
fn latency_sensitive_paths_stay_budgeted_and_input_first() -> TestResult<()> {
    let tui_path = manifest_path().join("src/tui.rs");
    let tui = fs::read_to_string(&tui_path)?;

    for phrase in [
        "const INPUT_POLL_TIMEOUT: Duration = Duration::from_millis(33);",
        "const QUEUED_INPUT_DRAIN_LIMIT: usize = 64;",
        "event::poll(INPUT_POLL_TIMEOUT)?",
        "drain_queued_input(app, peek_toggle_keys.as_ref(), &mut peek_prefix_pending).await?",
        "event::poll(Duration::from_millis(0))?",
        "for _ in 0..QUEUED_INPUT_DRAIN_LIMIT",
    ] {
        assert_contains(&tui_path, &tui, phrase)?;
    }
    assert_not_contains(&tui_path, &tui, "Duration::from_millis(200)")?;
    assert_ordered(
        &tui_path,
        &tui,
        &[
            "terminal.draw(|frame| draw_with_profile(frame, app, terminal_profile))?",
            "if event::poll(INPUT_POLL_TIMEOUT)?",
            "drain_queued_input(app, peek_toggle_keys.as_ref(), &mut peek_prefix_pending).await?",
            "} else {\n            app.tick().await?;",
        ],
    )?;

    let app_path = manifest_path().join("src/app.rs");
    let app = fs::read_to_string(&app_path)?;
    for phrase in [
        "const DIRTY_CAPTURE_LIMIT_PER_TICK: usize = 2;",
        "self.take_dirty_pane_batch(DIRTY_CAPTURE_LIMIT_PER_TICK)",
        "fn take_dirty_pane_batch(&mut self, limit: usize) -> Vec<String>",
    ] {
        assert_contains(&app_path, &app, phrase)?;
    }
    assert_not_contains(&app_path, &app, "self.dirty_pane_ids.drain()")?;

    Ok(())
}

#[test]
fn direct_navigation_actions_stay_in_memory_and_synchronous() -> TestResult<()> {
    let app_path = manifest_path().join("src/app.rs");
    let app = fs::read_to_string(&app_path)?;
    let latency_sensitive_methods = [
        "    pub fn select_next_pane(&mut self)",
        "    pub fn select_previous_pane(&mut self)",
        "    pub fn go_back(&mut self) -> bool",
        "    pub fn cycle_panel_focus(&mut self)",
        "    pub fn clear_view_scope(&mut self)",
        "    pub fn begin_search(&mut self)",
        "    pub fn push_search_char(&mut self, ch: char)",
        "    pub fn pop_search_char(&mut self)",
        "    pub fn finish_search(&mut self)",
        "    pub fn cancel_search(&mut self) -> bool",
        "    pub fn begin_command_input(&mut self)",
        "    pub fn push_command_char(&mut self, ch: char)",
        "    pub fn pop_command_char(&mut self)",
        "    pub fn cancel_command_input(&mut self) -> bool",
        "    pub fn command_input_can_repeat_recent(&self) -> bool",
        "    pub fn begin_launch_input(&mut self)",
        "    pub fn push_launch_char(&mut self, ch: char)",
        "    pub fn pop_launch_char(&mut self)",
        "    pub fn cancel_launch_input(&mut self) -> bool",
        "    pub fn begin_group_save_input(&mut self)",
        "    pub fn push_group_name_char(&mut self, ch: char)",
        "    pub fn pop_group_name_char(&mut self)",
        "    pub fn cancel_group_input(&mut self) -> bool",
        "    pub fn open_action_menu(&mut self)",
        "    pub fn toggle_selected_mark(&mut self)",
    ];
    let banned_work = [
        ".await",
        "tmux::",
        "capture_pane_tail",
        "refresh_metrics",
        "std::process",
        "tokio::process",
        "Command::new(",
        "read_to_string",
        "write(",
        "fs::",
    ];

    for signature in latency_sensitive_methods {
        let span = source_section_until_next_pub_fn(&app, signature)
            .ok_or_else(|| format!("{} must contain `{signature}`", app_path.display()))?;
        assert_not_contains_any(&app_path, span, &banned_work)?;
    }

    Ok(())
}

#[test]
fn ux_action_recipes_exercise_real_key_and_tmux_actions() -> TestResult<()> {
    let justfile_path = manifest_path().join("justfile");
    let justfile = fs::read_to_string(&justfile_path)?;
    let ux_actions = recipe_body(&justfile, "ux-actions")
        .ok_or_else(|| format!("{} must define `ux-actions`", justfile_path.display()))?;
    for phrase in [
        "cargo test --lib usability_action_contract -- --nocapture",
        "cargo test --lib app_tmux_action_paths_are_exercised_against_fake_tmux_binary -- --nocapture",
    ] {
        assert_contains(&justfile_path, ux_actions, phrase)?;
    }

    let ux_live_actions = recipe_body(&justfile, "ux-live-actions")
        .ok_or_else(|| format!("{} must define `ux-live-actions`", justfile_path.display()))?;
    for phrase in [
        "enter_opens_output_without_exiting_and_jump_keeps_muxboard_running",
        "same_server_enter_keeps_muxboard_visible_and_jump_leaves_it_running",
        "same_server_jump_handles_cross_session_targets",
        "manual_refresh_survives_target_tmux_server_disappearing",
        "manual_refresh_reconnects_live_updates_after_tmux_reappears",
        "output_panel_shows_real_tmux_tail_before_metadata",
        "output_panel_updates_while_open_after_real_pane_output",
        "refresh_recovers_from_stale_waiting_output_after_a_real_state_change",
        "live_status_update_replaces_stale_latest_and_next",
        "opening_output_marks_explicit_agent_review_seen_live",
        "smart_action_sends_enter_to_a_waiting_pane",
        "free_form_reply_journey_uses_reply_copy_and_dispatches_live",
        "single_target_send_labels_enter_send_and_dispatches_immediately",
        "review_send_cancel_keeps_targets_safe_and_recovers_cleanly",
        "review_send_survives_target_pane_disappearing_before_confirm",
        "review_send_recovers_when_every_target_pane_disappears_before_confirm",
        "summary_action_sends_one_line_prompt_to_live_tmux",
        "zoom_action_toggles_live_tmux_pane_without_leaving_muxboard",
        "search_mark_and_confirmed_multi_send_work_against_live_tmux",
        "saved_target_group_can_be_reloaded_and_used_for_broadcast",
        "stale_saved_fleet_stays_recoverable_after_live_pane_disappears",
        "action_menu_clear_marks_resets_targeting_to_the_selected_pane",
        "action_menu_can_acknowledge_and_restore_selected_attention",
        "action_menu_uses_rebound_secondary_keys",
        "notification_settings_persist_across_restart_and_stay_ssh_safe",
        "launch_agent_creates_new_tmux_window_without_leaving_muxboard",
        "launch_agent_recovers_when_target_server_disappears",
        "command_center_escape_returns_to_fleet_details_in_live_tmux",
        "browse_escape_returns_to_fleet_details_in_live_tmux",
        "browse_enter_scopes_to_live_window_and_backspace_recovers",
        "command_center_primary_action_continues_waiting_agent",
        "command_center_primary_action_answers_choice_prompt",
    ] {
        assert_contains(&justfile_path, ux_live_actions, phrase)?;
    }

    let ux_live_surfaces = recipe_body(&justfile, "ux-live-surfaces")
        .ok_or_else(|| format!("{} must define `ux-live-surfaces`", justfile_path.display()))?;
    for phrase in [
        "narrow_terminal_keeps_the_board_scannable",
        "ssh_like_dumb_terminal_keeps_the_board_legible",
        "fleet_keeps_plain_session_window_locations_readable_live",
        "idle_shell_prompt_noise_stays_out_of_fleet_latest",
        "visible_agent_thinking_state_is_running_not_idle_live",
        "shell_prompt_after_agent_activity_is_idle_not_running_live",
        "first_screen_prioritizes_attention_and_hides_secondary_details",
        "command_center_large_attention_queue_shows_overflow_live",
    ] {
        assert_contains(&justfile_path, ux_live_surfaces, phrase)?;
    }

    let ux_live_startup = recipe_body(&justfile, "ux-live-startup")
        .ok_or_else(|| format!("{} must define `ux-live-startup`", justfile_path.display()))?;
    for phrase in [
        "no_tmux_server_first_run_explains_recovery",
        "missing_session_first_run_explains_recovery",
        "invalid_config_falls_back_to_defaults_and_still_starts",
    ] {
        assert_contains(&justfile_path, ux_live_startup, phrase)?;
    }

    let ux_live_persistence = recipe_body(&justfile, "ux-live-persistence").ok_or_else(|| {
        format!(
            "{} must define `ux-live-persistence`",
            justfile_path.display()
        )
    })?;
    for phrase in [
        "acknowledgement_persists_across_restart",
        "saved_group_persists_across_restart_and_can_be_reloaded",
    ] {
        assert_contains(&justfile_path, ux_live_persistence, phrase)?;
    }

    let ux_live_navigation = recipe_body(&justfile, "ux-live-navigation").ok_or_else(|| {
        format!(
            "{} must define `ux-live-navigation`",
            justfile_path.display()
        )
    })?;
    for phrase in [
        "search_cancel_restores_the_previous_filter",
        "target_set_stays_obvious_while_selection_moves",
        "small_board_scrolls_to_keep_deep_selections_visible",
    ] {
        assert_contains(&justfile_path, ux_live_navigation, phrase)?;
    }

    let ux_live_churn = recipe_body(&justfile, "ux-live-churn")
        .ok_or_else(|| format!("{} must define `ux-live-churn`", justfile_path.display()))?;
    for phrase in [
        "resize_churn_preserves_selection_and_attention_context",
        "carriage_return_progress_updates_follow_visible_pane_state",
        "multi_pane_churn_keeps_attention_current",
    ] {
        assert_contains(&justfile_path, ux_live_churn, phrase)?;
    }

    let dogfood = recipe_body(&justfile, "dogfood")
        .ok_or_else(|| format!("{} must define `dogfood`", justfile_path.display()))?;
    assert_contains(&justfile_path, dogfood, "just tmux-plugin-live")?;
    assert_contains(&justfile_path, dogfood, "just ux-live-actions")?;
    assert_contains(&justfile_path, dogfood, "just ux-live-surfaces")?;
    assert_contains(&justfile_path, dogfood, "just ux-live-startup")?;
    assert_contains(&justfile_path, dogfood, "just ux-live-persistence")?;
    assert_contains(&justfile_path, dogfood, "just ux-live-navigation")?;
    assert_contains(&justfile_path, dogfood, "just ux-live-churn")?;
    assert_contains(&justfile_path, dogfood, "just perf-live")?;

    let testing_matrix_path = manifest_path().join("docs/testing-matrix.md");
    let testing_matrix = fs::read_to_string(&testing_matrix_path)?;
    for phrase in [
        "`just ux-live-actions` covers real keypresses",
        "`just ux-live-surfaces` covers first-screen hierarchy",
        "`just ux-live-startup` covers recoverable first-run",
        "`just ux-live-persistence` covers restart-backed state",
        "`just ux-live-navigation` covers filters",
        "`just ux-live-churn` covers resize",
        "`just dogfood` runs the named live recipes plus live performance",
    ] {
        assert_contains(&testing_matrix_path, &testing_matrix, phrase)?;
    }

    Ok(())
}

#[test]
fn live_dispatch_tests_prove_muxboard_survives_send_actions() -> TestResult<()> {
    let live_e2e_path = manifest_path().join("tests/live_e2e.rs");
    let live_e2e = fs::read_to_string(&live_e2e_path)?;

    assert_contains(
        &live_e2e_path,
        &live_e2e,
        "fn assert_muxboard_still_running",
    )?;
    assert_contains(&live_e2e_path, &live_e2e, "#{pane_current_command}")?;

    for signature in [
        "fn search_mark_and_confirmed_multi_send_work_against_live_tmux()",
        "fn single_target_send_labels_enter_send_and_dispatches_immediately()",
        "fn review_send_cancel_keeps_targets_safe_and_recovers_cleanly()",
        "fn review_send_survives_target_pane_disappearing_before_confirm()",
        "fn review_send_recovers_when_every_target_pane_disappears_before_confirm()",
    ] {
        let span = source_section_until_next_test(&live_e2e, signature)
            .ok_or_else(|| format!("{} must contain `{signature}`", live_e2e_path.display()))?;
        assert_contains(&live_e2e_path, span, "assert_muxboard_still_running")?;
    }

    Ok(())
}

#[test]
fn live_smart_action_tests_wait_for_selected_action_state() -> TestResult<()> {
    let live_e2e_path = manifest_path().join("tests/live_e2e.rs");
    let live_e2e = fs::read_to_string(&live_e2e_path)?;
    let lines = live_e2e.lines().collect::<Vec<_>>();

    assert_contains(&live_e2e_path, &live_e2e, "fn wait_for_selected_action(")?;

    let generic_action_waits = lines
        .iter()
        .enumerate()
        .filter_map(|(line_number, line)| {
            let is_generic_action_wait =
                line.contains(".wait_for_text(") && line.contains("\"Action: continue\"");
            is_generic_action_wait.then_some(format!("{}: {}", line_number + 1, line.trim()))
        })
        .collect::<Vec<_>>();
    if !generic_action_waits.is_empty() {
        return Err(format!(
            "{} must wait for smart actions by selected pane and action together:\n{}",
            live_e2e_path.display(),
            generic_action_waits.join("\n")
        )
        .into());
    }

    for signature in [
        "fn smart_action_sends_enter_to_a_waiting_pane()",
        "fn lane_smart_action_sends_enter_to_waiting_agents_only()",
    ] {
        let span = source_section_until_next_test(&live_e2e, signature)
            .ok_or_else(|| format!("{} must contain `{signature}`", live_e2e_path.display()))?;
        assert_contains(&live_e2e_path, span, "wait_for_selected_action(")?;
    }

    Ok(())
}

#[test]
fn live_setup_tests_wait_for_selected_rows_before_actions() -> TestResult<()> {
    let live_e2e_path = manifest_path().join("tests/live_e2e.rs");
    let live_e2e = fs::read_to_string(&live_e2e_path)?;
    let lines = live_e2e.lines().collect::<Vec<_>>();

    assert_contains(&live_e2e_path, &live_e2e, "fn wait_for_selected_row(")?;
    let helper_start = live_e2e.find("fn wait_for_selected_row(").ok_or_else(|| {
        format!(
            "{} must contain selected-row helper",
            live_e2e_path.display()
        )
    })?;
    let helper_tail = &live_e2e[helper_start..];
    let helper_end = helper_tail
        .find("\nfn wait_for_fleet_action_feedback(")
        .unwrap_or(helper_tail.len());
    let selected_row_helper = &helper_tail[..helper_end];
    assert_contains(
        &live_e2e_path,
        selected_row_helper,
        "screen.contains(\"Fleet\")",
    )?;
    assert_contains(
        &live_e2e_path,
        selected_row_helper,
        "screen.contains(\"Details\")",
    )?;

    let generic_location_waits = lines
        .iter()
        .enumerate()
        .filter_map(|(line_number, line)| {
            let is_generic_location_wait = line.contains(".wait_for_text(")
                && (line.contains("\"ops/prompt\"")
                    || line.contains("\"ops/split\"")
                    || line.contains("\"review/prompt\""));
            is_generic_location_wait.then_some(format!("{}: {}", line_number + 1, line.trim()))
        })
        .collect::<Vec<_>>();
    if !generic_location_waits.is_empty() {
        return Err(format!(
            "{} must wait for selected rows instead of generic location text before acting:\n{}",
            live_e2e_path.display(),
            generic_location_waits.join("\n")
        )
        .into());
    }

    for signature in [
        "fn single_target_send_labels_enter_send_and_dispatches_immediately()",
        "fn summary_action_sends_one_line_prompt_to_live_tmux()",
        "fn zoom_action_toggles_live_tmux_pane_without_leaving_muxboard()",
        "fn manual_refresh_survives_target_tmux_server_disappearing()",
        "fn manual_refresh_reconnects_live_updates_after_tmux_reappears()",
        "fn same_server_jump_handles_cross_session_targets()",
    ] {
        let span = source_section_until_next_test(&live_e2e, signature)
            .ok_or_else(|| format!("{} must contain `{signature}`", live_e2e_path.display()))?;
        assert_contains(&live_e2e_path, span, "wait_for_selected_row(")?;
    }

    Ok(())
}

#[test]
fn live_more_action_feedback_waits_are_surface_specific() -> TestResult<()> {
    let live_e2e_path = manifest_path().join("tests/live_e2e.rs");
    let live_e2e = fs::read_to_string(&live_e2e_path)?;
    let lines = live_e2e.lines().collect::<Vec<_>>();

    assert_contains(
        &live_e2e_path,
        &live_e2e,
        "fn wait_for_fleet_action_feedback(",
    )?;

    let generic_feedback_waits = lines
        .iter()
        .enumerate()
        .filter_map(|(line_number, line)| {
            let is_generic_feedback_wait = line.contains(".wait_for_text(")
                && (line.contains("\"Asked 1 pane for a one-line summary\"")
                    || line.contains("\"Toggled zoom\"")
                    || line.contains("\"Lane send enabled\""));
            is_generic_feedback_wait.then_some(format!("{}: {}", line_number + 1, line.trim()))
        })
        .collect::<Vec<_>>();
    if !generic_feedback_waits.is_empty() {
        return Err(format!(
            "{} must wait for top-level Fleet feedback instead of generic post-action text:\n{}",
            live_e2e_path.display(),
            generic_feedback_waits.join("\n")
        )
        .into());
    }

    for signature in [
        "fn summary_action_sends_one_line_prompt_to_live_tmux()",
        "fn zoom_action_toggles_live_tmux_pane_without_leaving_muxboard()",
        "fn lane_smart_action_sends_enter_to_waiting_agents_only()",
    ] {
        let span = source_section_until_next_test(&live_e2e, signature)
            .ok_or_else(|| format!("{} must contain `{signature}`", live_e2e_path.display()))?;
        assert_contains(&live_e2e_path, span, "wait_for_fleet_action_feedback(")?;
    }

    Ok(())
}

#[test]
fn live_first_board_tests_wait_for_main_board_surface() -> TestResult<()> {
    let live_e2e_path = manifest_path().join("tests/live_e2e.rs");
    let live_e2e = fs::read_to_string(&live_e2e_path)?;

    assert_contains(&live_e2e_path, &live_e2e, "fn wait_for_main_board_surface(")?;

    for (signature, forbidden_waits) in [
        (
            "fn first_screen_prioritizes_attention_and_hides_secondary_details()",
            &["\"Waiting\""][..],
        ),
        (
            "fn narrow_terminal_keeps_the_board_scannable()",
            &["\"Action:\""][..],
        ),
        (
            "fn ssh_like_dumb_terminal_keeps_the_board_legible()",
            &["\"Action:\""][..],
        ),
    ] {
        let span = source_section_until_next_test(&live_e2e, signature)
            .ok_or_else(|| format!("{} must contain `{signature}`", live_e2e_path.display()))?;
        assert_contains(&live_e2e_path, span, "wait_for_main_board_surface(")?;
        for forbidden_wait in forbidden_waits {
            let raw_wait = format!(".wait_for_text(&driver_pane, {forbidden_wait})");
            assert_not_contains(&live_e2e_path, span, &raw_wait)?;
        }
    }

    Ok(())
}

#[test]
fn live_first_run_recovery_tests_wait_for_recovery_surface() -> TestResult<()> {
    let live_e2e_path = manifest_path().join("tests/live_e2e.rs");
    let live_e2e = fs::read_to_string(&live_e2e_path)?;

    assert_contains(&live_e2e_path, &live_e2e, "fn wait_for_recovery_surface(")?;

    for (signature, forbidden_wait) in [
        (
            "fn no_tmux_server_first_run_explains_recovery()",
            "\"No tmux server.\"",
        ),
        (
            "fn missing_session_first_run_explains_recovery()",
            "\"Session not found.\"",
        ),
    ] {
        let span = source_section_until_next_test(&live_e2e, signature)
            .ok_or_else(|| format!("{} must contain `{signature}`", live_e2e_path.display()))?;
        assert_contains(&live_e2e_path, span, "wait_for_recovery_surface(")?;
        let raw_wait = format!(".wait_for_text(&driver_pane, {forbidden_wait})");
        assert_not_contains(&live_e2e_path, span, &raw_wait)?;
    }

    Ok(())
}

#[test]
fn live_e2e_default_helpers_do_not_trip_theme_onboarding() -> TestResult<()> {
    let live_e2e_path = manifest_path().join("tests/live_e2e.rs");
    let live_e2e = fs::read_to_string(&live_e2e_path)?;

    assert_contains(&live_e2e_path, &live_e2e, "const LIVE_TEST_BASE_CONFIG")?;
    assert_contains(&live_e2e_path, &live_e2e, "\"CatppuccinLatte\"")?;
    assert_contains(
        &live_e2e_path,
        &live_e2e,
        "fn isolated_muxboard_command_without_config(",
    )?;

    for signature in [
        "fn isolated_muxboard_command(",
        "fn isolated_muxboard_command_with_env(",
        "fn isolated_muxboard_command_with_config(",
        "fn isolated_muxboard_environment_with_config(",
    ] {
        let span = source_section_until_next_test(&live_e2e, signature)
            .ok_or_else(|| format!("{} must contain `{signature}`", live_e2e_path.display()))?;
        assert_contains(&live_e2e_path, span, "LIVE_TEST_BASE_CONFIG")?;
    }

    for signature in [
        "fn no_tmux_server_first_run_explains_recovery()",
        "fn missing_session_first_run_explains_recovery()",
    ] {
        let span = source_section_until_next_test(&live_e2e, signature)
            .ok_or_else(|| format!("{} must contain `{signature}`", live_e2e_path.display()))?;
        assert_contains(
            &live_e2e_path,
            span,
            "isolated_muxboard_command_without_config(",
        )?;
    }

    Ok(())
}

#[test]
fn live_provider_state_tests_wait_for_main_board_surface() -> TestResult<()> {
    let live_e2e_path = manifest_path().join("tests/live_e2e.rs");
    let live_e2e = fs::read_to_string(&live_e2e_path)?;

    for (signature, forbidden_wait) in [
        (
            "fn fleet_keeps_plain_session_window_locations_readable_live()",
            "\"muxdog/claude\"",
        ),
        (
            "fn idle_shell_prompt_noise_stays_out_of_fleet_latest()",
            "\"quiet muxboard: ready\"",
        ),
        (
            "fn visible_agent_thinking_state_is_running_not_idle_live()",
            "\"State: Running\"",
        ),
        (
            "fn shell_prompt_after_agent_activity_is_idle_not_running_live()",
            "\"Action: ready\"",
        ),
    ] {
        let span = source_section_until_next_test(&live_e2e, signature)
            .ok_or_else(|| format!("{} must contain `{signature}`", live_e2e_path.display()))?;
        assert_contains(&live_e2e_path, span, "wait_for_main_board_surface(")?;
        let raw_wait = format!(".wait_for_text(&driver_pane, {forbidden_wait})");
        assert_not_contains(&live_e2e_path, span, &raw_wait)?;
    }

    Ok(())
}

#[test]
fn live_resize_and_navigation_tests_wait_for_main_board_surface() -> TestResult<()> {
    let live_e2e_path = manifest_path().join("tests/live_e2e.rs");
    let live_e2e = fs::read_to_string(&live_e2e_path)?;

    for (signature, forbidden_waits) in [
        (
            "fn resize_churn_preserves_selection_and_attention_context()",
            &["\"1 needs you\"", "\">+ ops/prompt\""][..],
        ),
        (
            "fn small_board_scrolls_to_keep_deep_selections_visible()",
            &["\"Fleet | 1-5 / 8 | all quiet\"", "\">+ ops/w8\""][..],
        ),
        (
            "fn large_fleet_navigation_holds_up_with_twenty_panes()",
            &["\"Fleet | 1-5 / 20 | all quiet\"", "\">+ ops/w20\""][..],
        ),
    ] {
        let span = source_section_until_next_test(&live_e2e, signature)
            .ok_or_else(|| format!("{} must contain `{signature}`", live_e2e_path.display()))?;
        assert_contains(&live_e2e_path, span, "wait_for_main_board_surface")?;
        if signature == "fn resize_churn_preserves_selection_and_attention_context()" {
            assert_contains(
                &live_e2e_path,
                span,
                "resize_window_and_wait_for_board_surface(",
            )?;
            assert_not_contains(&live_e2e_path, span, "resize_window_and_wait_for_surface(")?;
        }
        for forbidden_wait in forbidden_waits {
            assert_not_contains(
                &live_e2e_path,
                span,
                &format!(".wait_for_text(&driver_pane, {forbidden_wait})"),
            )?;
            assert_not_contains(
                &live_e2e_path,
                span,
                &format!(
                    ".wait_for_text_with_poll(\n        &driver_pane,\n        {forbidden_wait}"
                ),
            )?;
        }
    }

    Ok(())
}

#[test]
fn live_launch_tests_wait_for_start_and_recovery_surfaces() -> TestResult<()> {
    let live_e2e_path = manifest_path().join("tests/live_e2e.rs");
    let live_e2e = fs::read_to_string(&live_e2e_path)?;

    assert_contains(
        &live_e2e_path,
        &live_e2e,
        "fn wait_for_start_agent_surface(",
    )?;
    assert_contains(&live_e2e_path, &live_e2e, "fn wait_for_launch_feedback(")?;

    let launch_span = source_section_until_next_test(
        &live_e2e,
        "fn launch_agent_creates_new_tmux_window_without_leaving_muxboard()",
    )
    .ok_or_else(|| {
        format!(
            "{} must contain launch success live test",
            live_e2e_path.display()
        )
    })?;
    assert_contains(&live_e2e_path, launch_span, "wait_for_start_agent_surface(")?;
    assert_contains(&live_e2e_path, launch_span, "wait_for_launch_feedback(")?;
    assert_not_contains(
        &live_e2e_path,
        launch_span,
        ".wait_for_text(&driver_pane, \"In: ops / agents\")",
    )?;
    assert_not_contains(
        &live_e2e_path,
        launch_span,
        ".wait_for_text(&driver_pane, \"Started `bash -lc\")",
    )?;

    let recovery_span = source_section_until_next_test(
        &live_e2e,
        "fn launch_agent_recovers_when_target_server_disappears()",
    )
    .ok_or_else(|| {
        format!(
            "{} must contain launch recovery live test",
            live_e2e_path.display()
        )
    })?;
    assert_contains(
        &live_e2e_path,
        recovery_span,
        "wait_for_start_agent_surface(",
    )?;
    assert_contains(&live_e2e_path, recovery_span, "wait_for_recovery_surface(")?;
    assert_not_contains(
        &live_e2e_path,
        recovery_span,
        ".wait_for_text(&driver_pane, \"In: ops / agents\")",
    )?;
    assert_not_contains(
        &live_e2e_path,
        recovery_span,
        ".wait_for_text(&driver_pane, \"No tmux server\")",
    )?;

    Ok(())
}

#[test]
fn live_acknowledgement_and_notification_tests_wait_for_board_state() -> TestResult<()> {
    let live_e2e_path = manifest_path().join("tests/live_e2e.rs");
    let live_e2e = fs::read_to_string(&live_e2e_path)?;

    for (signature, required_helpers, forbidden_waits) in [
        (
            "fn action_menu_can_acknowledge_and_restore_selected_attention()",
            &["wait_for_main_board_surface("][..],
            &["\"Waiting\""][..],
        ),
        (
            "fn notification_settings_persist_across_restart_and_stay_ssh_safe()",
            &[
                "wait_for_main_board_surface(",
                "wait_for_fleet_action_feedback(",
            ][..],
            &["\"1 needs you\"", "\"Desktop alerts off.\""][..],
        ),
        (
            "fn acknowledgement_persists_across_restart()",
            &["wait_for_main_board_surface("][..],
            &[
                "\"Waiting\"",
                "\"Fleet | 1-1 / 1 | all quiet\"",
                "\"1 needs you\"",
            ][..],
        ),
    ] {
        let span = source_section_until_next_test(&live_e2e, signature)
            .ok_or_else(|| format!("{} must contain `{signature}`", live_e2e_path.display()))?;
        for helper in required_helpers {
            assert_contains(&live_e2e_path, span, helper)?;
        }
        for forbidden_wait in forbidden_waits {
            assert_not_contains(
                &live_e2e_path,
                span,
                &format!(".wait_for_text(&driver_pane, {forbidden_wait})"),
            )?;
        }
    }

    Ok(())
}

#[test]
fn live_review_dispatch_result_tests_wait_for_result_surface() -> TestResult<()> {
    let live_e2e_path = manifest_path().join("tests/live_e2e.rs");
    let live_e2e = fs::read_to_string(&live_e2e_path)?;

    assert_contains(
        &live_e2e_path,
        &live_e2e,
        "fn wait_for_review_dispatch_result(",
    )?;

    for (signature, forbidden_wait) in [
        (
            "fn review_send_survives_target_pane_disappearing_before_confirm()",
            "\"1 pane disappeared\"",
        ),
        (
            "fn review_send_recovers_when_every_target_pane_disappears_before_confirm()",
            "\"No panes remain\"",
        ),
    ] {
        let span = source_section_until_next_test(&live_e2e, signature)
            .ok_or_else(|| format!("{} must contain `{signature}`", live_e2e_path.display()))?;
        assert_contains(&live_e2e_path, span, "wait_for_review_dispatch_result(")?;
        assert_not_contains(
            &live_e2e_path,
            span,
            &format!(".wait_for_text(&driver_pane, {forbidden_wait})"),
        )?;
    }

    Ok(())
}

#[test]
fn live_jump_tests_prove_muxboard_survives_and_targets_pane() -> TestResult<()> {
    let live_e2e_path = manifest_path().join("tests/live_e2e.rs");
    let live_e2e = fs::read_to_string(&live_e2e_path)?;

    for signature in [
        "fn same_server_jump_handles_cross_session_targets()",
        "fn run_output_or_jump_flow(target: &TmuxServer, should_jump: bool)",
        "fn run_same_server_output_or_jump_flow(should_jump: bool)",
    ] {
        let span = source_section_until_next_test(&live_e2e, signature)
            .ok_or_else(|| format!("{} must contain `{signature}`", live_e2e_path.display()))?;
        assert_contains(&live_e2e_path, span, "assert_muxboard_still_running")?;
        assert_contains(&live_e2e_path, span, "#{pane_active}")?;
    }

    Ok(())
}

#[test]
fn live_output_tests_wait_for_the_output_surface_not_the_generic_heading() -> TestResult<()> {
    let live_e2e_path = manifest_path().join("tests/live_e2e.rs");
    let live_e2e = fs::read_to_string(&live_e2e_path)?;

    assert_contains(
        &live_e2e_path,
        &live_e2e,
        "fn wait_for_output_surface(server: &TmuxServer, pane: &str) -> TestResult<String>",
    )?;

    let output_signature =
        "fn wait_for_output_surface(server: &TmuxServer, pane: &str) -> TestResult<String>";
    let output_start = live_e2e.find(output_signature).ok_or_else(|| {
        format!(
            "{} must contain `{output_signature}`",
            live_e2e_path.display()
        )
    })?;
    let output_tail = &live_e2e[output_start + output_signature.len()..];
    let output_end = output_tail
        .find("\nfn ")
        .map(|offset| output_start + output_signature.len() + offset)
        .unwrap_or(live_e2e.len());
    let output_helper = &live_e2e[output_start..output_end];
    assert_not_contains(&live_e2e_path, output_helper, ".wait_for_text(")?;
    for required in [
        "Output",
        "Esc back",
        "Enter output",
        "Enter details",
        "Command Center",
        "Browse",
    ] {
        assert_contains(&live_e2e_path, output_helper, required)?;
    }

    for signature in [
        "fn output_panel_updates_while_open_after_real_pane_output()",
        "fn run_output_or_jump_flow(target: &TmuxServer, should_jump: bool)",
        "fn run_same_server_output_or_jump_flow(should_jump: bool)",
    ] {
        let span = source_section_until_next_test(&live_e2e, signature)
            .ok_or_else(|| format!("{} must contain `{signature}`", live_e2e_path.display()))?;
        assert_contains(&live_e2e_path, span, "wait_for_output_surface(")?;
    }

    let fragile_waits =
        live_e2e
            .lines()
            .enumerate()
            .filter_map(|(line_number, line)| {
                (line.contains(".wait_for_text(") && line.contains("\"Output\""))
                    .then_some(format!("{}: {}", line_number + 1, line.trim()))
            })
            .collect::<Vec<_>>();
    if !fragile_waits.is_empty() {
        return Err(format!(
            "{} must not wait for the generic `Output` heading because Details renders it too:\n{}",
            live_e2e_path.display(),
            fragile_waits.join("\n")
        )
        .into());
    }

    Ok(())
}

#[test]
fn live_tests_wait_for_muxboard_process_not_command_echo() -> TestResult<()> {
    let live_e2e_path = manifest_path().join("tests/live_e2e.rs");
    let live_e2e = fs::read_to_string(&live_e2e_path)?;

    assert_contains(
        &live_e2e_path,
        &live_e2e,
        "fn wait_for_muxboard_surface(server: &TmuxServer, pane: &str) -> TestResult<String>",
    )?;
    assert_contains(&live_e2e_path, &live_e2e, "#{pane_current_command}")?;

    let fragile_waits =
        live_e2e
            .lines()
            .enumerate()
            .filter_map(|(line_number, line)| {
                (line.contains(".wait_for_text(") && line.contains("\"muxboard\""))
                    .then_some(format!("{}: {}", line_number + 1, line.trim()))
            })
            .collect::<Vec<_>>();
    if !fragile_waits.is_empty() {
        return Err(format!(
            "{} must not wait for generic `muxboard` text because the launch command can echo it first:\n{}",
            live_e2e_path.display(),
            fragile_waits.join("\n")
        )
        .into());
    }

    Ok(())
}

#[test]
fn live_driver_ui_waits_use_named_surface_helpers() -> TestResult<()> {
    let live_e2e_path = manifest_path().join("tests/live_e2e.rs");
    let live_e2e = fs::read_to_string(&live_e2e_path)?;

    for helper in [
        "fn wait_for_main_board_surface_without(",
        "fn wait_for_output_surface_with_text_with_poll(",
        "fn wait_for_live_status_summary_with_poll(",
        "fn wait_for_help_surface(",
        "fn wait_for_command_center_surface(",
        "fn wait_for_browse_surface(",
        "fn wait_for_browse_scope(",
    ] {
        assert_contains(&live_e2e_path, &live_e2e, helper)?;
    }

    let raw_driver_waits = live_e2e
        .lines()
        .enumerate()
        .filter_map(|(line_number, line)| {
            let waits_on_driver_ui = line.contains(".wait_for_text(&driver_pane")
                || line.contains("driver.wait_for_text_with_poll(");
            waits_on_driver_ui.then_some(format!("{}: {}", line_number + 1, line.trim()))
        })
        .collect::<Vec<_>>();
    if !raw_driver_waits.is_empty() {
        return Err(format!(
            "{} must wait for named muxboard UI surfaces instead of raw driver-pane text:\n{}",
            live_e2e_path.display(),
            raw_driver_waits.join("\n")
        )
        .into());
    }

    Ok(())
}

#[test]
fn live_raw_text_waits_are_external_target_sentinels() -> TestResult<()> {
    let live_e2e_path = manifest_path().join("tests/live_e2e.rs");
    let live_e2e = fs::read_to_string(&live_e2e_path)?;

    let non_target_waits = live_e2e
        .lines()
        .enumerate()
        .filter_map(|(line_number, line)| {
            let raw_wait = line.contains(".wait_for_text(");
            let external_target_wait = line.contains("target.wait_for_text(&");
            (raw_wait && !external_target_wait).then_some(format!(
                "{}: {}",
                line_number + 1,
                line.trim()
            ))
        })
        .collect::<Vec<_>>();
    if !non_target_waits.is_empty() {
        return Err(format!(
            "{} raw wait_for_text calls must remain external target-pane sentinels; muxboard UI waits need named surface helpers:\n{}",
            live_e2e_path.display(),
            non_target_waits.join("\n")
        )
        .into());
    }

    Ok(())
}

#[test]
fn live_more_menu_tests_wait_for_visible_rows_before_selecting() -> TestResult<()> {
    let live_e2e_path = manifest_path().join("tests/live_e2e.rs");
    let live_e2e = fs::read_to_string(&live_e2e_path)?;
    let lines = live_e2e.lines().collect::<Vec<_>>();

    assert_contains(
        &live_e2e_path,
        &live_e2e,
        "fn wait_for_more_row(server: &TmuxServer, pane: &str, row_text: &str) -> TestResult<String>",
    )?;

    let fragile_opens = lines
        .iter()
        .enumerate()
        .filter_map(|(line_index, line)| {
            if !(line.contains(".send_keys(") && line.contains("&[\".\"]")) {
                return None;
            }
            let next_lines = lines
                .iter()
                .skip(line_index + 1)
                .take(3)
                .copied()
                .collect::<Vec<_>>();
            next_lines
                .iter()
                .all(|next| !next.contains("wait_for_more_row("))
                .then(|| {
                    format!(
                        "{}: {}\n{}",
                        line_index + 1,
                        line.trim(),
                        next_lines.join("\n")
                    )
                })
        })
        .collect::<Vec<_>>();
    if !fragile_opens.is_empty() {
        return Err(format!(
            "{} must wait for visible More rows immediately after `.`:\n{}",
            live_e2e_path.display(),
            fragile_opens.join("\n\n")
        )
        .into());
    }

    Ok(())
}

#[test]
fn live_send_tests_wait_for_visible_send_surfaces_before_typing() -> TestResult<()> {
    let live_e2e_path = manifest_path().join("tests/live_e2e.rs");
    let live_e2e = fs::read_to_string(&live_e2e_path)?;
    let lines = live_e2e.lines().collect::<Vec<_>>();

    for helper in [
        "fn wait_for_send_surface(server: &TmuxServer, pane: &str) -> TestResult<String>",
        "fn wait_for_reply_surface(",
        "fn wait_for_review_surface(",
        "fn wait_for_inert_send_key(",
    ] {
        assert_contains(&live_e2e_path, &live_e2e, helper)?;
    }

    let review_signature = "fn wait_for_review_surface(";
    let review_start = live_e2e.find(review_signature).ok_or_else(|| {
        format!(
            "{} must contain `{review_signature}`",
            live_e2e_path.display()
        )
    })?;
    let review_tail = &live_e2e[review_start + review_signature.len()..];
    let review_end = review_tail
        .find("\nfn ")
        .map(|offset| review_start + review_signature.len() + offset)
        .unwrap_or(live_e2e.len());
    let review_helper = &live_e2e[review_start..review_end];
    assert_not_contains(&live_e2e_path, review_helper, ".wait_for_text(")?;
    for required in ["target_text", "Enter send", "Esc cancel", "Send to", "More"] {
        assert_contains(&live_e2e_path, review_helper, required)?;
    }

    let fragile_opens = lines
        .iter()
        .enumerate()
        .filter_map(|(line_index, line)| {
            if !(line.contains(".send_keys(") && line.contains("&[\":\"]")) {
                return None;
            }
            let next_lines = lines
                .iter()
                .skip(line_index + 1)
                .take(4)
                .copied()
                .collect::<Vec<_>>();
            next_lines
                .iter()
                .all(|next| {
                    !next.contains("wait_for_send_surface(")
                        && !next.contains("wait_for_reply_surface(")
                        && !next.contains("wait_for_inert_send_key(")
                })
                .then(|| {
                    format!(
                        "{}: {}\n{}",
                        line_index + 1,
                        line.trim(),
                        next_lines.join("\n")
                    )
                })
        })
        .collect::<Vec<_>>();
    if !fragile_opens.is_empty() {
        return Err(format!(
            "{} must wait for visible Send surfaces immediately after `:`:\n{}",
            live_e2e_path.display(),
            fragile_opens.join("\n\n")
        )
        .into());
    }

    Ok(())
}

#[test]
fn live_send_command_text_waits_stay_inside_send_surface() -> TestResult<()> {
    let live_e2e_path = manifest_path().join("tests/live_e2e.rs");
    let live_e2e = fs::read_to_string(&live_e2e_path)?;
    let lines = live_e2e.lines().collect::<Vec<_>>();

    assert_contains(&live_e2e_path, &live_e2e, "fn wait_for_send_command_text(")?;
    assert_contains(&live_e2e_path, &live_e2e, "fn wait_for_reply_command_text(")?;

    let fragile_send_literals = lines
        .iter()
        .enumerate()
        .filter_map(|(line_index, line)| {
            if !line.contains(".send_literal(&driver_pane") {
                return None;
            }
            let previous_lines = lines
                .iter()
                .take(line_index)
                .rev()
                .take(4)
                .copied()
                .collect::<Vec<_>>();
            let is_send_surface_literal = previous_lines.iter().any(|previous| {
                previous.contains("wait_for_send_surface(")
                    || previous.contains("wait_for_reply_surface(")
            });
            if !is_send_surface_literal {
                return None;
            }
            let next_lines = lines
                .iter()
                .skip(line_index + 1)
                .take(4)
                .copied()
                .collect::<Vec<_>>();
            next_lines
                .iter()
                .all(|next| {
                    !next.contains("wait_for_send_command_text(")
                        && !next.contains("wait_for_reply_command_text(")
                })
                .then(|| {
                    format!(
                        "{}: {}\n{}",
                        line_index + 1,
                        line.trim(),
                        next_lines.join("\n")
                    )
                })
        })
        .collect::<Vec<_>>();
    if !fragile_send_literals.is_empty() {
        return Err(format!(
            "{} must wait for typed Send command text inside the Send surface, not with generic pane text waits:\n{}",
            live_e2e_path.display(),
            fragile_send_literals.join("\n\n")
        )
        .into());
    }

    Ok(())
}

#[test]
fn live_send_list_targeting_waits_include_selected_rows() -> TestResult<()> {
    let live_e2e_path = manifest_path().join("tests/live_e2e.rs");
    let live_e2e = fs::read_to_string(&live_e2e_path)?;
    let lines = live_e2e.lines().collect::<Vec<_>>();

    assert_contains(
        &live_e2e_path,
        &live_e2e,
        "fn wait_for_send_list_target_state(",
    )?;
    let target_clarity_span = source_section_until_next_test(
        &live_e2e,
        "fn target_set_stays_obvious_while_selection_moves()",
    )
    .ok_or_else(|| {
        format!(
            "{} must contain `target_set_stays_obvious_while_selection_moves`",
            live_e2e_path.display()
        )
    })?;
    assert_contains(
        &live_e2e_path,
        target_clarity_span,
        "wait_for_main_board_surface(",
    )?;
    assert_not_contains(
        &live_e2e_path,
        target_clarity_span,
        "wait_for_line_with_texts(",
    )?;

    let fragile_space_toggles = lines
        .iter()
        .enumerate()
        .filter_map(|(line_index, line)| {
            if !(line.contains(".send_keys(") && line.contains("&[\"Space\"]")) {
                return None;
            }
            let next_lines = lines
                .iter()
                .skip(line_index + 1)
                .take(3)
                .copied()
                .collect::<Vec<_>>();
            next_lines
                .iter()
                .all(|next| !next.contains("wait_for_send_list_target_state("))
                .then(|| {
                    format!(
                        "{}: {}\n{}",
                        line_index + 1,
                        line.trim(),
                        next_lines.join("\n")
                    )
                })
        })
        .collect::<Vec<_>>();
    if !fragile_space_toggles.is_empty() {
        return Err(format!(
            "{} must wait for send-list counts plus the visible selected targeted row after Space:\n{}",
            live_e2e_path.display(),
            fragile_space_toggles.join("\n\n")
        )
        .into());
    }

    let fragile_count_waits = lines
        .iter()
        .enumerate()
        .filter_map(|(line_number, line)| {
            let is_generic_send_list_count_wait = line.contains(".wait_for_text(")
                && (line.contains("\"send list (1 pane)\"")
                    || line.contains("\"send list (2 panes)\""));
            is_generic_send_list_count_wait.then_some(format!(
                "{}: {}",
                line_number + 1,
                line.trim()
            ))
        })
        .collect::<Vec<_>>();
    if !fragile_count_waits.is_empty() {
        return Err(format!(
            "{} must not wait for send-list counts without also proving the selected row state:\n{}",
            live_e2e_path.display(),
            fragile_count_waits.join("\n")
        )
        .into());
    }

    Ok(())
}

#[test]
fn live_secondary_surface_tests_wait_for_real_surfaces() -> TestResult<()> {
    let live_e2e_path = manifest_path().join("tests/live_e2e.rs");
    let live_e2e = fs::read_to_string(&live_e2e_path)?;
    let lines = live_e2e.lines().collect::<Vec<_>>();

    for helper in [
        "fn wait_for_command_center_surface(",
        "fn wait_for_browse_surface(",
    ] {
        assert_contains(&live_e2e_path, &live_e2e, helper)?;
    }

    for (key, helper, label) in [
        (
            "&[\"]\"]",
            "wait_for_command_center_surface(",
            "Command Center",
        ),
        ("&[\"[\"]", "wait_for_browse_surface(", "Browse"),
    ] {
        let fragile_opens = lines
            .iter()
            .enumerate()
            .filter_map(|(line_index, line)| {
                if !(line.contains(".send_keys(") && line.contains(key)) {
                    return None;
                }
                let next_lines = lines
                    .iter()
                    .skip(line_index + 1)
                    .take(3)
                    .copied()
                    .collect::<Vec<_>>();
                next_lines
                    .iter()
                    .all(|next| !next.contains(helper))
                    .then(|| {
                        format!(
                            "{}: {}\n{}",
                            line_index + 1,
                            line.trim(),
                            next_lines.join("\n")
                        )
                    })
            })
            .collect::<Vec<_>>();
        if !fragile_opens.is_empty() {
            return Err(format!(
                "{} must wait for the real {label} surface immediately after opening it:\n{}",
                live_e2e_path.display(),
                fragile_opens.join("\n\n")
            )
            .into());
        }
    }

    let generic_heading_waits = lines
        .iter()
        .enumerate()
        .filter_map(|(line_number, line)| {
            let is_generic_heading_wait = line.contains(".wait_for_text(")
                && (line.contains("\"Command Center\"") || line.contains("\"Browse\""));
            is_generic_heading_wait.then_some(format!("{}: {}", line_number + 1, line.trim()))
        })
        .collect::<Vec<_>>();
    if !generic_heading_waits.is_empty() {
        return Err(format!(
            "{} must not wait for secondary surfaces by generic heading alone:\n{}",
            live_e2e_path.display(),
            generic_heading_waits.join("\n")
        )
        .into());
    }

    Ok(())
}

#[test]
fn live_search_tests_wait_for_visible_search_state() -> TestResult<()> {
    let live_e2e_path = manifest_path().join("tests/live_e2e.rs");
    let live_e2e = fs::read_to_string(&live_e2e_path)?;
    let lines = live_e2e.lines().collect::<Vec<_>>();

    for helper in [
        "fn wait_for_search_input_surface(",
        "fn wait_for_search_result(",
    ] {
        assert_contains(&live_e2e_path, &live_e2e, helper)?;
    }

    let fragile_inline_queries = lines
        .iter()
        .enumerate()
        .filter_map(|(line_index, line)| {
            if !line.contains("&[\"/\",") {
                return None;
            }
            let next_lines = lines
                .iter()
                .skip(line_index + 1)
                .take(5)
                .copied()
                .collect::<Vec<_>>();
            next_lines
                .iter()
                .all(|next| !next.contains("wait_for_search_result("))
                .then(|| {
                    format!(
                        "{}: {}\n{}",
                        line_index + 1,
                        line.trim(),
                        next_lines.join("\n")
                    )
                })
        })
        .collect::<Vec<_>>();
    if !fragile_inline_queries.is_empty() {
        return Err(format!(
            "{} must wait for applied search results after typing `/...Enter`:\n{}",
            live_e2e_path.display(),
            fragile_inline_queries.join("\n\n")
        )
        .into());
    }

    let fragile_search_inputs = lines
        .iter()
        .enumerate()
        .filter_map(|(line_index, line)| {
            if !(line.contains(".send_keys(") && line.contains("&[\"/\"]")) {
                return None;
            }
            let next_lines = lines
                .iter()
                .skip(line_index + 1)
                .take(3)
                .copied()
                .collect::<Vec<_>>();
            next_lines
                .iter()
                .all(|next| !next.contains("wait_for_search_input_surface("))
                .then(|| {
                    format!(
                        "{}: {}\n{}",
                        line_index + 1,
                        line.trim(),
                        next_lines.join("\n")
                    )
                })
        })
        .collect::<Vec<_>>();
    if !fragile_search_inputs.is_empty() {
        return Err(format!(
            "{} must wait for the visible Search input surface immediately after `/`:\n{}",
            live_e2e_path.display(),
            fragile_search_inputs.join("\n\n")
        )
        .into());
    }

    let generic_search_waits = lines
        .iter()
        .enumerate()
        .filter_map(|(line_number, line)| {
            let is_generic_search_wait = line.contains(".wait_for_text(")
                && (line.contains("\"search: ")
                    || line.contains("\"type to filter\"")
                    || line.contains("\"backspace show all\""));
            is_generic_search_wait.then_some(format!("{}: {}", line_number + 1, line.trim()))
        })
        .collect::<Vec<_>>();
    if !generic_search_waits.is_empty() {
        return Err(format!(
            "{} must not use generic search waits that can pass on stale input, footer, or header text:\n{}",
            live_e2e_path.display(),
            generic_search_waits.join("\n")
        )
        .into());
    }

    Ok(())
}

#[test]
fn live_e2e_tests_use_named_waits_instead_of_inline_fixed_sleeps() -> TestResult<()> {
    let live_e2e_path = manifest_path().join("tests/live_e2e.rs");
    let live_e2e = fs::read_to_string(&live_e2e_path)?;

    for helper in [
        "fn resize_window_and_wait_for_board_surface(",
        "fn wait_for_board_surface(",
        "fn wait_for_screen_with_texts_without(",
        "fn wait_for_output_surface_to_stay_open(",
        "fn type_literal_slowly(",
    ] {
        assert_contains(&live_e2e_path, &live_e2e, helper)?;
    }
    assert_not_contains(&live_e2e_path, &live_e2e, "wait_for_line_with_texts(")?;

    let inline_sleeps = live_e2e
        .lines()
        .enumerate()
        .filter_map(|(line_number, line)| {
            line.contains("thread::sleep(Duration::").then_some(format!(
                "{}: {}",
                line_number + 1,
                line.trim()
            ))
        })
        .collect::<Vec<_>>();
    if !inline_sleeps.is_empty() {
        return Err(format!(
            "{} must use named state waits instead of inline fixed sleeps:\n{}",
            live_e2e_path.display(),
            inline_sleeps.join("\n")
        )
        .into());
    }

    Ok(())
}

#[test]
fn live_escape_tests_wait_for_dismissed_surfaces_to_disappear() -> TestResult<()> {
    let live_e2e_path = manifest_path().join("tests/live_e2e.rs");
    let live_e2e = fs::read_to_string(&live_e2e_path)?;
    let lines = live_e2e.lines().collect::<Vec<_>>();

    for helper in [
        "fn wait_for_stale_fleet_board_after_picker_escape(",
        "fn wait_for_output_escape_returns_to_details(",
        "fn wait_for_help_escape_returns_to_details(",
        "fn wait_for_command_center_escape_returns_to_details(",
        "fn wait_for_browse_escape_returns_to_details(",
    ] {
        assert_contains(&live_e2e_path, &live_e2e, helper)?;
    }
    assert_not_contains(&live_e2e_path, &live_e2e, "wait_for_screen_without_text(")?;

    let allowed_waits = [
        "wait_for_search_result(",
        "wait_for_send_list_target_state(",
        "wait_for_stale_fleet_board_after_picker_escape(",
        "wait_for_output_escape_returns_to_details(",
        "wait_for_help_escape_returns_to_details(",
        "wait_for_command_center_escape_returns_to_details(",
        "wait_for_browse_escape_returns_to_details(",
    ];

    let fragile_escapes = lines
        .iter()
        .enumerate()
        .filter_map(|(line_index, line)| {
            if !(line.contains(".send_keys(") && line.contains("&[\"Escape\"]")) {
                return None;
            }
            let next_lines = lines
                .iter()
                .skip(line_index + 1)
                .take(8)
                .copied()
                .collect::<Vec<_>>();
            next_lines
                .iter()
                .all(|next| !allowed_waits.iter().any(|wait| next.contains(wait)))
                .then(|| {
                    format!(
                        "{}: {}\n{}",
                        line_index + 1,
                        line.trim(),
                        next_lines.join("\n")
                    )
                })
        })
        .collect::<Vec<_>>();
    if !fragile_escapes.is_empty() {
        return Err(format!(
            "{} must use named surface waits after Escape, not generic disappearance checks:\n{}",
            live_e2e_path.display(),
            fragile_escapes.join("\n\n")
        )
        .into());
    }

    Ok(())
}

#[test]
fn live_clear_send_list_tests_wait_for_visible_reset() -> TestResult<()> {
    let live_e2e_path = manifest_path().join("tests/live_e2e.rs");
    let live_e2e = fs::read_to_string(&live_e2e_path)?;
    let lines = live_e2e.lines().collect::<Vec<_>>();

    assert_contains(
        &live_e2e_path,
        &live_e2e,
        "fn wait_for_clear_send_list_action(",
    )?;

    let fragile_clears = lines
        .iter()
        .enumerate()
        .filter_map(|(line_index, line)| {
            if !(line.contains(".send_keys(") && line.contains("&[\"x\"]")) {
                return None;
            }
            let next_lines = lines
                .iter()
                .skip(line_index + 1)
                .take(3)
                .copied()
                .collect::<Vec<_>>();
            next_lines
                .iter()
                .all(|next| !next.contains("wait_for_clear_send_list_action("))
                .then(|| {
                    format!(
                        "{}: {}\n{}",
                        line_index + 1,
                        line.trim(),
                        next_lines.join("\n")
                    )
                })
        })
        .collect::<Vec<_>>();
    if !fragile_clears.is_empty() {
        return Err(format!(
            "{} must wait for the visible clear-send-list reset after `X clear`:\n{}",
            live_e2e_path.display(),
            fragile_clears.join("\n\n")
        )
        .into());
    }

    Ok(())
}

#[test]
fn live_saved_fleet_tests_wait_for_picker_and_active_state() -> TestResult<()> {
    let live_e2e_path = manifest_path().join("tests/live_e2e.rs");
    let live_e2e = fs::read_to_string(&live_e2e_path)?;
    let lines = live_e2e.lines().collect::<Vec<_>>();

    for helper in [
        "fn wait_for_save_fleet_input(",
        "fn wait_for_saved_fleet_picker(",
        "fn wait_for_saved_fleet_active(",
    ] {
        assert_contains(&live_e2e_path, &live_e2e, helper)?;
    }

    let fragile_save_opens = lines
        .iter()
        .enumerate()
        .filter_map(|(line_index, line)| {
            if !(line.contains(".send_keys(") && line.contains("&[\"g\"]")) {
                return None;
            }
            let previous_lines = lines
                .iter()
                .take(line_index)
                .rev()
                .take(3)
                .copied()
                .collect::<Vec<_>>();
            let is_save_fleet_action = previous_lines.iter().any(|previous| {
                previous.contains("wait_for_more_row(") && previous.contains("save fleet")
            });
            if !is_save_fleet_action {
                return None;
            }
            let next_lines = lines
                .iter()
                .skip(line_index + 1)
                .take(3)
                .copied()
                .collect::<Vec<_>>();
            next_lines
                .iter()
                .all(|next| !next.contains("wait_for_save_fleet_input("))
                .then(|| {
                    format!(
                        "{}: {}\n{}",
                        line_index + 1,
                        line.trim(),
                        next_lines.join("\n")
                    )
                })
        })
        .collect::<Vec<_>>();
    if !fragile_save_opens.is_empty() {
        return Err(format!(
            "{} must wait for the visible Save Fleet input immediately after `G save fleet`:\n{}",
            live_e2e_path.display(),
            fragile_save_opens.join("\n\n")
        )
        .into());
    }

    let fragile_picker_opens = lines
        .iter()
        .enumerate()
        .filter_map(|(line_index, line)| {
            if !(line.contains(".send_keys(") && line.contains("&[\"l\"]")) {
                return None;
            }
            let next_lines = lines
                .iter()
                .skip(line_index + 1)
                .take(3)
                .copied()
                .collect::<Vec<_>>();
            next_lines
                .iter()
                .all(|next| !next.contains("wait_for_saved_fleet_picker("))
                .then(|| {
                    format!(
                        "{}: {}\n{}",
                        line_index + 1,
                        line.trim(),
                        next_lines.join("\n")
                    )
                })
        })
        .collect::<Vec<_>>();
    if !fragile_picker_opens.is_empty() {
        return Err(format!(
            "{} must wait for the visible saved-fleet picker immediately after `L choose fleet`:\n{}",
            live_e2e_path.display(),
            fragile_picker_opens.join("\n\n")
        )
        .into());
    }

    let fragile_picker_loads = lines
        .iter()
        .enumerate()
        .filter_map(|(line_index, line)| {
            if !(line.contains("wait_for_saved_fleet_picker(") && line.contains("true")) {
                return None;
            }
            let next_lines = lines
                .iter()
                .skip(line_index + 1)
                .take(4)
                .copied()
                .collect::<Vec<_>>();
            next_lines
                .iter()
                .all(|next| !next.contains("wait_for_saved_fleet_active("))
                .then(|| {
                    format!(
                        "{}: {}\n{}",
                        line_index + 1,
                        line.trim(),
                        next_lines.join("\n")
                    )
                })
        })
        .collect::<Vec<_>>();
    if !fragile_picker_loads.is_empty() {
        return Err(format!(
            "{} must wait for the loaded fleet target and closed picker after loading a live saved fleet:\n{}",
            live_e2e_path.display(),
            fragile_picker_loads.join("\n\n")
        )
        .into());
    }

    let fragile_generic_waits = lines
        .iter()
        .enumerate()
        .filter_map(|(line_number, line)| {
            let is_generic_fleet_wait = line.contains(".wait_for_text(")
                && (line.contains("\"Fleets\"")
                    || line.contains("\"Send: fleet")
                    || line.contains("\"Save this send list"));
            is_generic_fleet_wait.then_some(format!("{}: {}", line_number + 1, line.trim()))
        })
        .collect::<Vec<_>>();
    if !fragile_generic_waits.is_empty() {
        return Err(format!(
            "{} must not use generic saved-fleet text waits that can pass on stale overlay or status text:\n{}",
            live_e2e_path.display(),
            fragile_generic_waits.join("\n")
        )
        .into());
    }

    Ok(())
}

#[test]
fn live_refresh_tests_wait_for_visible_refresh_results() -> TestResult<()> {
    let live_e2e_path = manifest_path().join("tests/live_e2e.rs");
    let live_e2e = fs::read_to_string(&live_e2e_path)?;
    let lines = live_e2e.lines().collect::<Vec<_>>();

    assert_contains(&live_e2e_path, &live_e2e, "fn wait_for_refresh_result(")?;
    let refresh_helper =
        source_section_until_next_function(&live_e2e, "fn wait_for_refresh_result(").ok_or_else(
            || {
                format!(
                    "{} must contain `wait_for_refresh_result`",
                    live_e2e_path.display()
                )
            },
        )?;
    assert_contains(&live_e2e_path, refresh_helper, "FORBIDDEN_REFRESH_SURFACES")?;
    assert_contains(
        &live_e2e_path,
        refresh_helper,
        "screen.contains(\"muxboard\")",
    )?;

    let fragile_refreshes = lines
        .iter()
        .enumerate()
        .filter_map(|(line_index, line)| {
            if !(line.contains(".send_keys(") && line.contains("&[\"r\"]")) {
                return None;
            }
            let next_lines = lines
                .iter()
                .skip(line_index + 1)
                .take(4)
                .copied()
                .collect::<Vec<_>>();
            next_lines
                .iter()
                .all(|next| !next.contains("wait_for_refresh_result("))
                .then(|| {
                    format!(
                        "{}: {}\n{}",
                        line_index + 1,
                        line.trim(),
                        next_lines.join("\n")
                    )
                })
        })
        .collect::<Vec<_>>();
    if !fragile_refreshes.is_empty() {
        return Err(format!(
            "{} must wait for a visible refresh result immediately after `R refresh`:\n{}",
            live_e2e_path.display(),
            fragile_refreshes.join("\n\n")
        )
        .into());
    }

    Ok(())
}

#[test]
fn live_stale_state_tests_wait_for_board_specific_surfaces() -> TestResult<()> {
    let live_e2e_path = manifest_path().join("tests/live_e2e.rs");
    let live_e2e = fs::read_to_string(&live_e2e_path)?;

    for (signature, required_helpers, forbidden_waits) in [
        (
            "fn refresh_recovers_from_stale_waiting_output_after_a_real_state_change()",
            &["wait_for_main_board_surface(", "wait_for_refresh_result("][..],
            &["\"1 needs you\""][..],
        ),
        (
            "fn live_status_update_replaces_stale_latest_and_next()",
            &[
                "wait_for_live_status_summary(",
                "wait_for_live_status_summary_with_poll(",
            ][..],
            &["\"old stale action\"", "\"ship fix\""][..],
        ),
        (
            "fn carriage_return_progress_updates_follow_visible_pane_state()",
            &["wait_for_main_board_surface("][..],
            &["\"ready\""][..],
        ),
        (
            "fn multi_pane_churn_keeps_attention_current()",
            &["wait_for_main_board_surface(", "wait_for_refresh_result("][..],
            &["\"2 need you\""][..],
        ),
    ] {
        let span = source_section_until_next_test(&live_e2e, signature)
            .ok_or_else(|| format!("{} must contain `{signature}`", live_e2e_path.display()))?;
        for helper in required_helpers {
            assert_contains(&live_e2e_path, span, helper)?;
        }
        for forbidden_wait in forbidden_waits {
            assert_not_contains(
                &live_e2e_path,
                span,
                &format!(".wait_for_text(&driver_pane, {forbidden_wait})"),
            )?;
            assert_not_contains(
                &live_e2e_path,
                span,
                &format!(
                    ".wait_for_text_with_poll(\n        &driver_pane,\n        {forbidden_wait}"
                ),
            )?;
        }
    }

    Ok(())
}

#[test]
fn dogfood_stays_aligned_with_non_perf_live_e2e_tests() -> TestResult<()> {
    let justfile_path = manifest_path().join("justfile");
    let justfile = fs::read_to_string(&justfile_path)?;
    let ux_live_actions = recipe_body(&justfile, "ux-live-actions")
        .ok_or_else(|| format!("{} must define `ux-live-actions`", justfile_path.display()))?;
    let ux_live_surfaces = recipe_body(&justfile, "ux-live-surfaces")
        .ok_or_else(|| format!("{} must define `ux-live-surfaces`", justfile_path.display()))?;
    let ux_live_startup = recipe_body(&justfile, "ux-live-startup")
        .ok_or_else(|| format!("{} must define `ux-live-startup`", justfile_path.display()))?;
    let ux_live_persistence = recipe_body(&justfile, "ux-live-persistence").ok_or_else(|| {
        format!(
            "{} must define `ux-live-persistence`",
            justfile_path.display()
        )
    })?;
    let ux_live_navigation = recipe_body(&justfile, "ux-live-navigation").ok_or_else(|| {
        format!(
            "{} must define `ux-live-navigation`",
            justfile_path.display()
        )
    })?;
    let ux_live_churn = recipe_body(&justfile, "ux-live-churn")
        .ok_or_else(|| format!("{} must define `ux-live-churn`", justfile_path.display()))?;
    let tmux_plugin_live = recipe_body(&justfile, "tmux-plugin-live")
        .ok_or_else(|| format!("{} must define `tmux-plugin-live`", justfile_path.display()))?;
    let dogfood = recipe_body(&justfile, "dogfood")
        .ok_or_else(|| format!("{} must define `dogfood`", justfile_path.display()))?;
    assert_contains(&justfile_path, dogfood, "just tmux-plugin-live")?;
    assert_contains(&justfile_path, dogfood, "just ux-live-actions")?;
    assert_contains(&justfile_path, dogfood, "just ux-live-surfaces")?;
    assert_contains(&justfile_path, dogfood, "just ux-live-startup")?;
    assert_contains(&justfile_path, dogfood, "just ux-live-persistence")?;
    assert_contains(&justfile_path, dogfood, "just ux-live-navigation")?;
    assert_contains(&justfile_path, dogfood, "just ux-live-churn")?;

    let live_e2e_path = manifest_path().join("tests/live_e2e.rs");
    let live_e2e = fs::read_to_string(&live_e2e_path)?;
    let covered_live_tests = format!(
        "{tmux_plugin_live}\n{ux_live_actions}\n{ux_live_surfaces}\n{ux_live_startup}\n{ux_live_persistence}\n{ux_live_navigation}\n{ux_live_churn}\n{dogfood}"
    );
    let dogfood_exemptions = ["large_fleet_navigation_holds_up_with_twenty_panes"];
    let missing = ignored_test_names(&live_e2e)
        .into_iter()
        .filter(|test_name| !dogfood_exemptions.contains(&test_name.as_str()))
        .filter(|test_name| !covered_live_tests.contains(test_name))
        .collect::<Vec<_>>();

    if !missing.is_empty() {
        return Err(format!(
            "non-perf live e2e tests must be covered by `just dogfood`: {missing:?}"
        )
        .into());
    }

    for test_name in dogfood_exemptions {
        assert_contains(&live_e2e_path, &live_e2e, &format!("fn {test_name}("))?;
    }

    Ok(())
}

#[test]
fn live_notification_settings_stay_covered_at_persistence_boundary() -> TestResult<()> {
    let test_name = "notification_settings_persist_across_restart_and_stay_ssh_safe";
    let live_e2e_path = manifest_path().join("tests/live_e2e.rs");
    let live_e2e = fs::read_to_string(&live_e2e_path)?;
    let span = source_section_until_next_test(&live_e2e, &format!("fn {test_name}("))
        .ok_or_else(|| format!("{} must contain `{test_name}`", live_e2e_path.display()))?;

    for required in [
        "isolated_muxboard_environment_with_config_and_env",
        "SSH_CONNECTION",
        "wait_for_notification_flags",
        "quit_muxboard_in_pane",
        "desktop alerts unavailable on SSH",
        "wait_for_notification_flags(&muxboard.config_file, true, false)",
        "wait_for_notification_flags(&muxboard.config_file, true, true)",
        "assert_muxboard_still_running",
    ] {
        assert_contains(&live_e2e_path, span, required)?;
    }
    assert_contains(&live_e2e_path, &live_e2e, "config_file: PathBuf")?;

    let justfile_path = manifest_path().join("justfile");
    let justfile = fs::read_to_string(&justfile_path)?;
    let ux_live_actions = recipe_body(&justfile, "ux-live-actions")
        .ok_or_else(|| format!("{} must define `ux-live-actions`", justfile_path.display()))?;
    assert_contains(&justfile_path, ux_live_actions, test_name)?;

    Ok(())
}

#[test]
fn packaged_project_avoids_local_identity_and_source_drop_markers() -> TestResult<()> {
    let local_home = format!("/Users/{}", "ali");
    let downloads = format!("~/{}", "Downloads");
    let source_drop = format!("cc_{}", "old");
    let banned = [
        local_home.as_str(),
        downloads.as_str(),
        source_drop.as_str(),
    ];

    for file in files_under_with_extension(".", None)? {
        if path_is_or_under(&file, ".git") || path_is_or_under(&file, "target") {
            continue;
        }

        let source = fs::read_to_string(&file).unwrap_or_default();
        assert_not_contains_any(&file, &source, &banned)?;
    }

    Ok(())
}

#[test]
fn mac_desktop_assumptions_stay_inside_notification_boundary() -> TestResult<()> {
    let allowed = ["src/notifications.rs"];
    let mac_desktop_markers = [
        "osascript",
        "TERM_PROGRAM",
        "target_os = \"macos\"",
        "Library/Application Support",
        "~/Library",
        "~/.muxboard",
    ];

    for file in rust_files_under("src")? {
        if allowed
            .iter()
            .any(|relative| path_is_or_under(&file, relative))
        {
            continue;
        }

        let source = fs::read_to_string(&file)?;
        assert_not_contains_any(&file, &source, &mac_desktop_markers)?;
    }

    for file in [
        manifest_path().join("README.md"),
        manifest_path().join("config.example.json"),
        manifest_path().join("docs/testing-matrix.md"),
    ] {
        let source = fs::read_to_string(&file)?;
        assert_not_contains_any(
            &file,
            &source,
            &["Library/Application Support", "~/Library", "~/.muxboard"],
        )?;
    }

    Ok(())
}

#[test]
fn architecture_file_walk_ignores_local_runtime_and_build_artifacts() {
    for relative in [
        ".git/config",
        ".hermes/auth.json",
        ".hermes/sessions/session.json",
        ".muxboard-agent/bin/agent-preflight.sh",
        "target/debug/muxboard",
    ] {
        assert!(
            path_is_local_worktree_artifact(&manifest_path().join(relative)),
            "{relative} must stay out of product file scans"
        );
    }

    for relative in ["src/main.rs", "tests/fixtures/tui/golden/help_overlay.txt"] {
        assert!(
            !path_is_local_worktree_artifact(&manifest_path().join(relative)),
            "{relative} is product content and should stay visible to guardrails"
        );
    }
}

#[test]
fn usability_golden_screens_keep_basic_wayfinding() -> TestResult<()> {
    for file in golden_screen_files()? {
        let source = fs::read_to_string(&file)?;
        let visible_lines = source
            .lines()
            .filter(|line| !line.trim().is_empty())
            .collect::<Vec<_>>();

        let first = visible_lines
            .first()
            .ok_or_else(|| format!("{} must not be empty", file.display()))?;
        let last = visible_lines
            .last()
            .ok_or_else(|| format!("{} must not be empty", file.display()))?;

        if !first.starts_with("muxboard") {
            return Err(format!(
                "{} must identify muxboard on the first visible line",
                file.display()
            )
            .into());
        }
        let file_name = file
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        let help_overlay_footer = file_name == "help_overlay.txt";
        let text_entry_footer = last.contains("type ") && last.contains("Esc cancel");
        if help_overlay_footer {
            if !last.starts_with("Esc close") || source.contains("? help") {
                return Err(format!(
                    "{} must show only Help recovery actions while Help is open",
                    file.display()
                )
                .into());
            }
        } else if text_entry_footer {
            if source.contains("? help") {
                return Err(format!(
                    "{} must not advertise ? help while ? is valid typed text",
                    file.display()
                )
                .into());
            }
        } else if !last.starts_with("? help") {
            return Err(format!("{} must leave the footer discoverable", file.display()).into());
        } else if source.matches("? help").count() != 1 {
            return Err(
                format!("{} must render exactly one help affordance", file.display()).into(),
            );
        }

        let empty_recovery_without_targets = source.contains("No matching panes.")
            && !source.contains("send list")
            && !source.contains("pane hidden");
        if empty_recovery_without_targets {
            assert_not_contains_any(
                &file,
                &source,
                &[
                    "J/K move",
                    "J/K browse",
                    "Enter output",
                    "Enter window",
                    "G show",
                    "Space add",
                    ": send",
                ],
            )?;
        }
    }

    Ok(())
}

#[test]
fn usability_golden_screens_avoid_internal_protocol_and_retired_words() -> TestResult<()> {
    let banned = [
        "NEXT=",
        "STATUS=",
        "BLOCKER=",
        "Next:",
        "Next step:",
        "target set",
        "target panes",
        "send target",
        "target hidden by current view",
        "targets hidden by current view",
        "No target panes remain",
        "Start target disappeared",
        "staged command",
        "staged send",
        "Selected pane",
        "Space target",
        "unknown",
        " unk",
        "Nothing here.",
        "1 attention",
        "2 attention",
        "3 attention",
        "4 attention",
        " / %",
    ];

    for file in golden_screen_files()? {
        let source = fs::read_to_string(&file)?;
        assert_not_contains_any(&file, &source, &banned)?;
    }

    Ok(())
}

#[test]
fn usability_golden_action_rows_stay_single_decision() -> TestResult<()> {
    for file in golden_screen_files()? {
        let source = fs::read_to_string(&file)?;
        for (line_index, line) in source.lines().enumerate() {
            if line.contains("Action:") && line.contains(" or ") {
                return Err(format!(
                    "{}:{} Action rows must name one primary decision, not competing alternatives: {}",
                    file.display(),
                    line_index + 1,
                    line.trim()
                )
                .into());
            }
        }
    }

    Ok(())
}

#[test]
fn usability_help_overlay_stays_task_oriented_not_a_footer_dump() -> TestResult<()> {
    let path = manifest_path().join("tests/fixtures/tui/golden/help_overlay.txt");
    let source = fs::read_to_string(&path)?;
    let required = [
        "Now:",
        "Send:",
        "Find:",
        "Move:",
        "More:",
        "Legend:",
        "Close:",
        "backspace show all",
        "Fleet/Details",
        "add/remove pane",
        ". then",
    ];
    let banned = [
        "J/K moves",
        "shows output",
        "opens Send",
        "opens more actions",
        "runs the shown",
        "Q quits.",
    ];
    let boxed_lines = source
        .lines()
        .filter(|line| line.contains('│'))
        .collect::<Vec<_>>();

    for term in required {
        assert_contains(&path, &source, term)?;
    }
    assert_not_contains_any(&path, &source, &banned)?;
    if boxed_lines.len() > 9 {
        return Err(format!(
            "{} help overlay should stay billboard-short, got {} boxed lines",
            path.display(),
            boxed_lines.len()
        )
        .into());
    }

    Ok(())
}

#[test]
fn usability_golden_screens_have_review_metadata() -> TestResult<()> {
    let manifest_path = manifest_path().join("tests/fixtures/tui/golden/manifest.json");
    let source = fs::read_to_string(&manifest_path)?;
    let entries: Vec<serde_json::Value> = serde_json::from_str(&source)?;
    let mut manifest_files = entries
        .iter()
        .map(|entry| {
            let file = entry
                .get("file")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| "golden manifest entry is missing file".to_string())?;
            let journey = entry
                .get("journey")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| format!("{file} is missing journey metadata"))?;
            if journey.trim().len() < 24 {
                return Err(format!("{file} journey metadata is too thin").into());
            }
            let protects = entry
                .get("protects")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| format!("{file} is missing protects metadata"))?;
            if protects.trim().len() < 48 {
                return Err(format!("{file} protects metadata is too thin").into());
            }
            let must_show = entry
                .get("must_show")
                .and_then(serde_json::Value::as_array)
                .ok_or_else(|| format!("{file} is missing must_show metadata"))?;
            if must_show.len() < 4 {
                return Err(
                    format!("{file} must_show should name the protected UX contract").into(),
                );
            }
            let must_not_show = entry
                .get("must_not_show")
                .and_then(serde_json::Value::as_array)
                .ok_or_else(|| format!("{file} is missing must_not_show metadata"))?;
            if must_not_show.len() < 3 {
                return Err(
                    format!("{file} must_not_show should protect against regressions").into(),
                );
            }
            let footer_must_show = entry
                .get("footer_must_show")
                .and_then(serde_json::Value::as_array)
                .ok_or_else(|| format!("{file} is missing footer_must_show metadata"))?;
            if footer_must_show.len() < 2 {
                return Err(
                    format!("{file} footer_must_show should protect recovery keys").into(),
                );
            }
            let max_boxed_lines = entry
                .get("max_boxed_lines")
                .and_then(serde_json::Value::as_u64)
                .ok_or_else(|| format!("{file} is missing max_boxed_lines metadata"))?;
            let golden = fs::read_to_string(
                manifest_path
                    .parent()
                    .expect("manifest should have parent")
                    .join(file),
            )?;
            for term in must_show {
                let term = term
                    .as_str()
                    .ok_or_else(|| format!("{file} must_show contains a non-string term"))?;
                assert_contains(
                    &manifest_path
                        .parent()
                        .expect("manifest should have parent")
                        .join(file),
                    &golden,
                    term,
                )?;
            }
            for term in must_not_show {
                let term = term
                    .as_str()
                    .ok_or_else(|| format!("{file} must_not_show contains a non-string term"))?;
                assert_not_contains(
                    &manifest_path
                        .parent()
                        .expect("manifest should have parent")
                        .join(file),
                    &golden,
                    term,
                )?;
            }
            let footer = last_visible_line(&golden)
                .ok_or_else(|| format!("{file} must render a visible footer"))?;
            for term in footer_must_show {
                let term = term
                    .as_str()
                    .ok_or_else(|| format!("{file} footer_must_show contains a non-string term"))?;
                if !footer.contains(term) {
                    return Err(format!("{file} footer must contain `{term}`:\n{footer}").into());
                }
            }
            let boxed_lines = golden.lines().filter(|line| line.contains('│')).count() as u64;
            if boxed_lines > max_boxed_lines {
                return Err(format!(
                    "{file} renders {boxed_lines} boxed lines, over max_boxed_lines {max_boxed_lines}"
                )
                .into());
            }
            Ok(String::from(file))
        })
        .collect::<TestResult<Vec<_>>>()?;
    manifest_files.sort();

    let mut golden_files = golden_screen_files()?
        .into_iter()
        .map(|file| {
            file.file_name()
                .and_then(|name| name.to_str())
                .map(String::from)
                .ok_or_else(|| format!("invalid golden file name: {}", file.display()).into())
        })
        .collect::<TestResult<Vec<_>>>()?;
    golden_files.sort();

    if manifest_files != golden_files {
        return Err(format!(
            "golden manifest mismatch\nmanifest: {manifest_files:?}\ngoldens: {golden_files:?}"
        )
        .into());
    }

    Ok(())
}

fn last_visible_line(source: &str) -> Option<&str> {
    source.lines().rev().find(|line| !line.trim().is_empty())
}

fn assert_no_terse_target_count_copy(file: &Path, source: &str) -> TestResult<()> {
    let is_rust = file.extension().and_then(|ext| ext.to_str()) == Some("rs");
    for (index, line) in source.lines().enumerate() {
        let visible_copy = if is_rust {
            rust_quoted_text(line)
        } else {
            line.to_owned()
        };
        if visible_copy.trim().is_empty() {
            continue;
        }
        let lowered = visible_copy.to_ascii_lowercase();
        for prefix in ["send list", "review"] {
            if let Some(phrase) = terse_space_count(&lowered, prefix) {
                return Err(format!(
                    "{}:{} uses terse target count `{phrase}` instead of `{phrase} pane(s)`: {}",
                    file.display(),
                    index + 1,
                    line.trim()
                )
                .into());
            }
        }
        if let Some(phrase) = bare_parenthesized_target_count(&lowered) {
            return Err(format!(
                "{}:{} uses terse target count `{phrase}` instead of `({} pane(s))`: {}",
                file.display(),
                index + 1,
                phrase.trim_matches(['(', ')']),
                line.trim()
            )
            .into());
        }
    }

    Ok(())
}

fn rust_quoted_text(line: &str) -> String {
    let mut segments = Vec::new();
    let mut current = String::new();
    let mut in_string = false;
    let mut escaped = false;

    for ch in line.chars() {
        if in_string {
            if escaped {
                current.push(ch);
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                segments.push(std::mem::take(&mut current));
                in_string = false;
            } else {
                current.push(ch);
            }
        } else if ch == '"' {
            in_string = true;
        }
    }

    if in_string && !current.is_empty() {
        segments.push(current);
    }

    segments.join(" ")
}

fn terse_space_count(line: &str, prefix: &str) -> Option<String> {
    let needle = format!("{prefix} ");
    let mut tail = line;

    while let Some(offset) = tail.find(&needle) {
        let count_start = offset + needle.len();
        let after_prefix = &tail[count_start..];
        let digit_count = after_prefix
            .chars()
            .take_while(|ch| ch.is_ascii_digit())
            .count();
        if digit_count == 0 {
            tail = &after_prefix[after_prefix
                .char_indices()
                .nth(1)
                .map(|(next, _)| next)
                .unwrap_or(after_prefix.len())..];
            continue;
        }

        let count = &after_prefix[..digit_count];
        let after_count = &after_prefix[digit_count..];
        if !starts_with_pane_unit(after_count) {
            return Some(format!("{prefix} {count}"));
        }
        tail = after_count;
    }

    None
}

fn starts_with_pane_unit(text: &str) -> bool {
    [" pane(s)", " panes", " pane"].into_iter().any(|unit| {
        text.strip_prefix(unit).is_some_and(|rest| {
            rest.chars()
                .next()
                .is_none_or(|ch| !ch.is_ascii_alphabetic())
        })
    })
}

fn bare_parenthesized_target_count(line: &str) -> Option<String> {
    let target_context = [
        "send list",
        "send to ",
        "review send",
        "fleet ",
        " lane",
        "lane ",
        "target set",
    ];
    if !target_context.iter().any(|context| line.contains(context)) {
        return None;
    }

    let mut tail = line;
    while let Some(open) = tail.find('(') {
        let after_open = &tail[open + 1..];
        let digit_count = after_open
            .chars()
            .take_while(|ch| ch.is_ascii_digit())
            .count();
        if digit_count > 0 && after_open[digit_count..].starts_with(')') {
            let count = &after_open[..digit_count];
            return Some(format!("({count})"));
        }
        tail = &after_open[after_open
            .char_indices()
            .nth(1)
            .map(|(next, _)| next)
            .unwrap_or(after_open.len())..];
    }

    None
}

fn primary_keybinding_label(defaults: &serde_json::Value, name: &str) -> TestResult<String> {
    let raw = defaults
        .get("keybindings")
        .and_then(|keybindings| keybindings.get(name))
        .and_then(serde_json::Value::as_array)
        .and_then(|bindings| bindings.first())
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| format!("default keybinding `{name}` is missing"))?;

    Ok(match raw {
        "enter" => String::from("Enter"),
        "space" => String::from("Space"),
        "tab" => String::from("Tab"),
        "backspace" => String::from("backspace"),
        _ => raw.to_owned(),
    })
}

#[test]
fn app_only_uses_core_through_the_top_level_api() -> TestResult<()> {
    let forbidden_core_paths = [
        "crate::core::inference",
        "crate::core::model",
        "crate::core::providers",
        "crate::core::reports",
        "crate::core::runtime",
        "core::inference",
        "core::model",
        "core::providers",
        "core::reports",
        "core::runtime",
        "core::{inference",
        "core::{model",
        "core::{providers",
        "core::{reports",
        "core::{runtime",
    ];

    for file in rust_files_under("src/app")?
        .into_iter()
        .chain(optional_file("src/app.rs"))
    {
        let source = fs::read_to_string(&file)?;
        assert_not_contains_any(&file, &source, &forbidden_core_paths)?;
    }

    Ok(())
}

#[test]
fn process_spawning_stays_in_io_boundary_modules() -> TestResult<()> {
    let allowed = ["src/tmux", "src/metrics.rs", "src/notifications.rs"];
    let process_markers = [
        "std::process::Command",
        "tokio::process::Command",
        "Command::new(",
    ];

    for file in rust_files_under("src")? {
        if allowed.iter().any(|prefix| path_is_or_under(&file, prefix)) {
            continue;
        }

        let source = fs::read_to_string(&file)?;
        assert_not_contains_any(&file, &source, &process_markers)?;
    }

    Ok(())
}

fn manifest_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn optional_file(relative: &str) -> Option<PathBuf> {
    let path = manifest_path().join(relative);
    path.exists().then_some(path)
}

fn rust_files_under(relative: &str) -> TestResult<Vec<PathBuf>> {
    files_under_with_extension(relative, Some("rs"))
}

fn golden_screen_files() -> TestResult<Vec<PathBuf>> {
    files_under_with_extension("tests/fixtures/tui/golden", Some("txt"))
}

fn files_under_with_extension(relative: &str, extension: Option<&str>) -> TestResult<Vec<PathBuf>> {
    let root = manifest_path().join(relative);
    if !root.exists() {
        return Ok(Vec::new());
    }

    let mut files = Vec::new();
    collect_files(&root, extension, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_files(path: &Path, extension: Option<&str>, files: &mut Vec<PathBuf>) -> TestResult<()> {
    if path_is_local_worktree_artifact(path) {
        return Ok(());
    }

    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let entry_path = entry.path();
        if entry_path.is_dir() {
            collect_files(&entry_path, extension, files)?;
        } else {
            let matches_extension = extension.is_none()
                || entry_path.extension().and_then(|ext| ext.to_str()) == extension;
            if matches_extension {
                files.push(entry_path);
            }
        }
    }

    Ok(())
}

fn path_is_local_worktree_artifact(file: &Path) -> bool {
    file.strip_prefix(manifest_path()).ok().is_some_and(|path| {
        path.components()
            .next()
            .and_then(|component| component.as_os_str().to_str())
            .is_some_and(|first| matches!(first, ".git" | ".hermes" | ".muxboard-agent" | "target"))
    })
}

fn path_is_or_under(file: &Path, relative: &str) -> bool {
    file.strip_prefix(manifest_path())
        .ok()
        .is_some_and(|path| path == Path::new(relative) || path.starts_with(relative))
}

fn recipe_body<'a>(source: &'a str, recipe: &str) -> Option<&'a str> {
    let parameterized_header = format!("{recipe} ");
    let mut start = None;
    let mut offset = 0;
    for line in source.split_inclusive('\n') {
        let trimmed = line.trim_start().trim_end();
        if trimmed.starts_with(&format!("{recipe}:")) || trimmed.starts_with(&parameterized_header)
        {
            let colon = line.find(':')?;
            start = Some(offset + colon + 1);
            break;
        }
        offset += line.len();
    }
    let start = start?;
    let tail = &source[start..];
    let end = tail
        .find("\n\n")
        .map(|offset| start + offset)
        .unwrap_or(source.len());
    Some(&source[start..end])
}

fn ignored_test_names(source: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut pending_ignored_test = false;

    for line in source.lines() {
        let line = line.trim();
        if line.starts_with("#[ignore") {
            pending_ignored_test = true;
            continue;
        }
        if !pending_ignored_test {
            continue;
        }
        if line.is_empty() || line.starts_with("#[") {
            continue;
        }
        if let Some(rest) = line.strip_prefix("fn ")
            && let Some((name, _)) = rest.split_once('(')
        {
            names.push(name.to_owned());
        }
        pending_ignored_test = false;
    }

    names
}

fn source_section_until_next_pub_fn<'a>(source: &'a str, signature: &str) -> Option<&'a str> {
    let start = source.find(signature)?;
    let tail_after_signature = &source[start + signature.len()..];
    let end = ["\n    pub fn ", "\n    pub async fn "]
        .into_iter()
        .filter_map(|needle| tail_after_signature.find(needle))
        .min()
        .map(|offset| start + signature.len() + offset)
        .unwrap_or(source.len());
    Some(&source[start..end])
}

fn source_section_until_next_test<'a>(source: &'a str, signature: &str) -> Option<&'a str> {
    let start = source.find(signature)?;
    let tail = &source[start + signature.len()..];
    let end = tail
        .find("\n#[test]")
        .map(|offset| start + signature.len() + offset)
        .unwrap_or(source.len());
    Some(&source[start..end])
}

fn source_section_until_next_function<'a>(source: &'a str, signature: &str) -> Option<&'a str> {
    let start = source.find(signature)?;
    let tail = &source[start + signature.len()..];
    let end = tail
        .find("\nfn ")
        .map(|offset| start + signature.len() + offset)
        .unwrap_or(source.len());
    Some(&source[start..end])
}

fn assert_not_contains(file: &Path, source: &str, needle: &str) -> TestResult<()> {
    if source.contains(needle) {
        return Err(format!("{} must not contain `{needle}`", file.display()).into());
    }

    Ok(())
}

fn assert_contains(file: &Path, source: &str, needle: &str) -> TestResult<()> {
    if !source.contains(needle) {
        return Err(format!("{} must contain `{needle}`", file.display()).into());
    }

    Ok(())
}

fn assert_ordered(file: &Path, source: &str, needles: &[&str]) -> TestResult<()> {
    let mut cursor = 0;

    for needle in needles {
        let tail = &source[cursor..];
        let Some(offset) = tail.find(needle) else {
            return Err(format!(
                "{} must contain `{needle}` after byte {cursor}",
                file.display()
            )
            .into());
        };
        cursor += offset + needle.len();
    }

    Ok(())
}

fn assert_png_dimensions(path: &Path, width: u32, height: u32) -> TestResult<()> {
    let bytes = fs::read(path)?;
    let signature = b"\x89PNG\r\n\x1a\n";
    if bytes.len() < 24 || &bytes[..8] != signature {
        return Err(format!("{} must be a PNG file", path.display()).into());
    }

    let actual_width = u32::from_be_bytes(bytes[16..20].try_into()?);
    let actual_height = u32::from_be_bytes(bytes[20..24].try_into()?);
    if (actual_width, actual_height) != (width, height) {
        return Err(format!(
            "{} must be {width}x{height}, got {actual_width}x{actual_height}",
            path.display()
        )
        .into());
    }

    Ok(())
}

fn assert_not_contains_any(file: &Path, source: &str, needles: &[&str]) -> TestResult<()> {
    for needle in needles {
        assert_not_contains(file, source, needle)?;
    }

    Ok(())
}
