use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    app::{KeyBindingsConfig, NotificationSettings, ThemeConfig, ThemePreset, UiSettings},
    paths,
};

#[derive(Debug, Clone)]
pub struct Store {
    path: PathBuf,
}

impl Store {
    pub fn new() -> Result<Self> {
        Ok(Self {
            path: paths::config_file()?,
        })
    }

    #[cfg(test)]
    pub(crate) fn new_at(path: PathBuf) -> Self {
        Self { path }
    }

    pub(crate) fn load_notification_settings(&self) -> Result<NotificationSettings> {
        Ok(self.load_config()?.notification_settings)
    }

    pub(crate) fn load_ui_settings(&self) -> Result<UiSettings> {
        let ui_settings = self.load_config()?.ui_settings;
        ui_settings.validate()?;
        Ok(ui_settings)
    }

    pub(crate) fn save_notification_settings(
        &self,
        notification_settings: &NotificationSettings,
    ) -> Result<()> {
        let mut config = self.load_config()?;
        config.notification_settings = notification_settings.clone();
        self.save_config(&config)
    }

    pub(crate) fn save_ui_settings(&self, ui_settings: &UiSettings) -> Result<()> {
        ui_settings.validate()?;
        let mut config = self.load_config()?;
        config.ui_settings = ui_settings.clone();
        self.save_config(&config)
    }

    pub(crate) fn save_theme_preset(&self, preset: ThemePreset) -> Result<()> {
        let mut config = self.load_config()?;
        config.ui_settings.theme_preset = ThemePreset::default();
        config.ui_settings.theme.preset = Some(preset);
        config.ui_settings.validate()?;
        self.save_config(&config)
    }

    pub(crate) fn should_show_theme_onboarding(&self) -> Result<bool> {
        if !self.path.exists() {
            return Ok(true);
        }

        let raw = fs::read_to_string(&self.path)
            .with_context(|| format!("failed to read {}", self.path.display()))?;
        let value: serde_json::Value = serde_json::from_str(&raw)
            .map_err(|error| anyhow::anyhow!("failed to parse muxboard config: {error}"))?;
        let theme_is_configured = value
            .get("ui_settings")
            .and_then(|settings| settings.as_object())
            .is_some_and(|settings| {
                settings.contains_key("theme") || settings.contains_key("theme_preset")
            });
        Ok(!theme_is_configured)
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    fn load_config(&self) -> Result<PersistedConfig> {
        if !self.path.exists() {
            return Ok(PersistedConfig::default());
        }

        let raw = fs::read_to_string(&self.path)
            .with_context(|| format!("failed to read {}", self.path.display()))?;
        serde_json::from_str(&raw)
            .map_err(|error| anyhow::anyhow!("failed to parse muxboard config: {error}"))
    }

    fn save_config(&self, config: &PersistedConfig) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }

        let json = serde_json::to_string_pretty(config).context("failed to serialize config")?;
        atomic_write(&self.path, &json)
    }
}

pub fn default_config_json() -> Result<String> {
    let config = PersistedConfig::example();
    serde_json::to_string_pretty(&config).context("failed to serialize default muxboard config")
}

pub fn default_keybindings_json() -> Result<String> {
    let keybindings = KeyBindingsConfig::default();
    serde_json::to_string_pretty(&json!({
        "layout_preset": UiSettings::default().layout_preset,
        "theme": ThemeConfig::example(),
        "keybindings": keybindings,
    }))
    .context("failed to serialize default muxboard keybindings")
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct PersistedConfig {
    notification_settings: NotificationSettings,
    ui_settings: UiSettings,
}

impl PersistedConfig {
    fn example() -> Self {
        let ui_settings = UiSettings {
            theme: ThemeConfig::example(),
            ..UiSettings::default()
        };

        Self {
            notification_settings: NotificationSettings::default(),
            ui_settings,
        }
    }
}

fn atomic_write(path: &Path, contents: &str) -> Result<()> {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system time should be valid")?
        .as_nanos();
    let parent = path
        .parent()
        .with_context(|| format!("{} has no parent directory", path.display()))?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .with_context(|| format!("{} has no valid file name", path.display()))?;
    let temp_path = parent.join(format!(".{file_name}.tmp-{}-{unique}", std::process::id()));

    fs::write(&temp_path, contents)
        .with_context(|| format!("failed to write {}", temp_path.display()))?;
    fs::rename(&temp_path, path)
        .with_context(|| format!("failed to replace {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Store, default_config_json, default_keybindings_json};
    use crate::app::{
        AlertPolicy, KeyBindingsConfig, LayoutPreset, NotificationSettings, ThemeColor,
        ThemeConfig, ThemeOverrides, ThemePreset, UiSettings,
    };
    use serde_json::Value;
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };

    static TEST_PATH_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn test_path() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should work")
            .as_nanos();
        std::env::temp_dir()
            .join(format!(
                "muxboard-config-test-{}-{}-{unique}",
                std::process::id(),
                TEST_PATH_COUNTER.fetch_add(1, Ordering::Relaxed)
            ))
            .join("config.json")
    }

    fn blocked_parent_path(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should work")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "muxboard-config-parent-blocked-{label}-{}-{}-{unique}",
            std::process::id(),
            TEST_PATH_COUNTER.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn missing_config_file_loads_defaults_and_exposes_path() {
        let path = test_path();
        let store = Store::new_at(path.clone());

        assert_eq!(store.path(), path.as_path());
        assert_eq!(
            store
                .load_notification_settings()
                .expect("missing notification settings should load"),
            NotificationSettings::default()
        );
        assert_eq!(
            store
                .load_ui_settings()
                .expect("missing UI settings should load"),
            UiSettings::default()
        );
    }

    #[test]
    fn malformed_config_file_returns_a_parse_error() {
        let path = test_path();
        fs::create_dir_all(path.parent().expect("parent should exist"))
            .expect("config dir should exist");
        fs::write(&path, "{ definitely not json").expect("config write should succeed");
        let store = Store::new_at(path.clone());

        let error = store
            .load_notification_settings()
            .expect_err("malformed config should fail");
        assert!(
            error
                .to_string()
                .contains("failed to parse muxboard config")
        );

        let _ = fs::remove_dir_all(path.parent().expect("parent should exist"));
    }

    #[test]
    fn store_round_trips_notification_settings() {
        let path = test_path();
        let store = Store::new_at(path.clone());
        let settings = NotificationSettings {
            bell_enabled: false,
            desktop_enabled: true,
            alert_policy: AlertPolicy::ErrorAndWaiting,
            debounce_seconds: 120,
        };

        store
            .save_notification_settings(&settings)
            .expect("save should succeed");
        let loaded = store
            .load_notification_settings()
            .expect("load should succeed");

        assert_eq!(loaded, settings);
        let _ = fs::remove_dir_all(path.parent().expect("parent should exist"));
    }

    #[test]
    fn save_notification_settings_reports_unusable_config_parent() {
        let blocked = blocked_parent_path("notifications");
        fs::write(&blocked, "not a directory").expect("blocking file should exist");
        let store = Store::new_at(blocked.join("config.json"));

        let error = store
            .save_notification_settings(&NotificationSettings::default())
            .expect_err("blocked config parent should fail");

        assert!(error.to_string().contains("failed to create"));
        let _ = fs::remove_file(blocked);
    }

    #[test]
    fn store_round_trips_ui_settings() {
        let path = test_path();
        let store = Store::new_at(path.clone());
        let ui_settings = UiSettings {
            layout_preset: LayoutPreset::Vertical,
            theme_preset: ThemePreset::Contrast,
            theme: ThemeConfig {
                preset: Some(ThemePreset::CatppuccinLatte),
                overrides: ThemeOverrides {
                    accent: Some(ThemeColor::Rgb(64, 120, 242)),
                    selected_bg: Some(ThemeColor::Indexed(24)),
                    ..ThemeOverrides::default()
                },
            },
            ..UiSettings::default()
        };

        store
            .save_ui_settings(&ui_settings)
            .expect("save should succeed");
        let loaded = store.load_ui_settings().expect("load should succeed");

        assert_eq!(loaded, ui_settings);
        let _ = fs::remove_dir_all(path.parent().expect("parent should exist"));
    }

    #[test]
    fn theme_config_preserves_legacy_theme_preset_and_accepts_overrides() {
        let legacy: UiSettings = serde_json::from_str(
            r##"{
  "layout_preset": "Auto",
  "theme_preset": "Contrast",
  "keybindings": {}
}"##,
        )
        .expect("legacy theme_preset should deserialize");

        assert_eq!(legacy.active_theme_preset(), ThemePreset::Contrast);
        assert_eq!(legacy.theme.preset, None);

        let modern: UiSettings = serde_json::from_str(
            r##"{
  "layout_preset": "Auto",
  "theme_preset": "Contrast",
  "theme": {
    "preset": "catppuccin-latte",
    "overrides": {
      "accent": "#4078F2",
      "muted": "bright black",
      "surface": "24"
    }
  },
  "keybindings": {}
}"##,
        )
        .expect("modern theme config should deserialize");

        assert_eq!(modern.active_theme_preset(), ThemePreset::CatppuccinLatte);
        assert_eq!(
            modern.theme.overrides.accent,
            Some(ThemeColor::Rgb(64, 120, 242))
        );
        assert_eq!(modern.theme.overrides.muted, Some(ThemeColor::DarkGray));
        assert_eq!(
            modern.theme.overrides.surface,
            Some(ThemeColor::Indexed(24))
        );
    }

    #[test]
    fn theme_presets_accept_builtin_palette_names() {
        for (raw, expected) in [
            ("Calm", ThemePreset::Calm),
            ("calm", ThemePreset::Calm),
            ("default", ThemePreset::TerminalNative),
            ("Contrast", ThemePreset::Contrast),
            ("high contrast", ThemePreset::Contrast),
            ("Mono", ThemePreset::Mono),
            ("no-color", ThemePreset::Mono),
            ("no_colour", ThemePreset::Mono),
            ("system", ThemePreset::TerminalNative),
            ("system-colors", ThemePreset::TerminalNative),
            ("system theme", ThemePreset::TerminalNative),
            ("terminal-native", ThemePreset::TerminalNative),
            ("Terminal Native", ThemePreset::TerminalNative),
            ("terminal", ThemePreset::TerminalNative),
            ("ansi", ThemePreset::TerminalNative),
            ("ansi terminal", ThemePreset::TerminalNative),
            ("light", ThemePreset::CatppuccinLatte),
            ("catppuccin-latte", ThemePreset::CatppuccinLatte),
            ("latte", ThemePreset::CatppuccinLatte),
            ("dark", ThemePreset::CatppuccinMocha),
            ("catppuccin-mocha", ThemePreset::CatppuccinMocha),
            ("Catppuccin Mocha", ThemePreset::CatppuccinMocha),
            ("catppuccin", ThemePreset::CatppuccinMocha),
            ("tokyo-night", ThemePreset::TokyoNight),
            ("tokyo night", ThemePreset::TokyoNight),
            ("TokyoNight", ThemePreset::TokyoNight),
            ("gruvbox-dark", ThemePreset::GruvboxDark),
            ("gruvbox", ThemePreset::GruvboxDark),
            ("gruvbox-light", ThemePreset::GruvboxLight),
            ("Gruvbox Light", ThemePreset::GruvboxLight),
            ("Nord", ThemePreset::Nord),
            ("rose-pine", ThemePreset::RosePine),
            ("RosePine", ThemePreset::RosePine),
            ("rosé pine", ThemePreset::RosePine),
            ("ROSÉ PINE", ThemePreset::RosePine),
        ] {
            let json = format!(
                r#"{{
  "theme": {{
    "preset": "{raw}"
  }},
  "keybindings": {{}}
}}"#
            );
            let ui_settings: UiSettings =
                serde_json::from_str(&json).expect("theme preset should deserialize");

            assert_eq!(ui_settings.active_theme_preset(), expected, "{raw}");
        }
    }

    #[test]
    fn theme_colors_parse_and_serialize_common_terminal_forms() {
        for (raw, expected, serialized) in [
            ("default", ThemeColor::Reset, "Reset"),
            ("bright black", ThemeColor::DarkGray, "DarkGray"),
            ("silver", ThemeColor::Gray, "Gray"),
            ("purple", ThemeColor::Magenta, "Magenta"),
            ("bright purple", ThemeColor::LightMagenta, "LightMagenta"),
            ("#4078f2", ThemeColor::Rgb(64, 120, 242), "#4078F2"),
            ("#abc", ThemeColor::Rgb(170, 187, 204), "#AABBCC"),
            ("42", ThemeColor::Indexed(42), "42"),
        ] {
            let color: ThemeColor =
                serde_json::from_str(&format!(r#""{raw}""#)).expect("theme color should parse");
            assert_eq!(color, expected, "{raw}");
            let encoded = serde_json::to_string(&color).expect("theme color should serialize");
            assert_eq!(encoded, format!(r#""{serialized}""#));
        }
    }

    #[test]
    fn load_ui_settings_rejects_bad_theme_colors_with_actionable_copy() {
        let path = test_path();
        let store = Store::new_at(path.clone());
        fs::create_dir_all(path.parent().expect("parent should exist"))
            .expect("config dir should exist");

        fs::write(
            &path,
            r#"{
  "ui_settings": {
    "theme": {
      "preset": "TerminalNative",
      "overrides": {
        "accent": "bluish"
      }
    }
  }
}"#,
        )
        .expect("config write should succeed");

        let error = store.load_ui_settings().expect_err("load should fail");
        let message = error.to_string();
        assert!(
            message.contains("invalid muxboard theme color `bluish`"),
            "{message}"
        );
        assert!(
            message.contains("named ANSI colors, 0-255, or #RGB/#RRGGBB"),
            "{message}"
        );

        let _ = fs::remove_dir_all(path.parent().expect("parent should exist"));
    }

    #[test]
    fn theme_onboarding_detects_missing_theme_without_overwriting_existing_config() {
        let path = test_path();
        let store = Store::new_at(path.clone());
        assert!(
            store
                .should_show_theme_onboarding()
                .expect("missing config should be readable")
        );

        fs::create_dir_all(path.parent().expect("parent should exist"))
            .expect("config dir should exist");
        fs::write(
            &path,
            r#"{
  "notification_settings": {
    "bell_enabled": false
  }
}"#,
        )
        .expect("config write should succeed");
        assert!(
            store
                .should_show_theme_onboarding()
                .expect("partial config should be readable")
        );

        store
            .save_theme_preset(ThemePreset::CatppuccinLatte)
            .expect("theme save should succeed");
        assert!(
            !store
                .should_show_theme_onboarding()
                .expect("saved theme should be readable")
        );

        let raw = fs::read_to_string(&path).expect("config should exist");
        let json: serde_json::Value = serde_json::from_str(&raw).expect("config should be JSON");
        assert_eq!(json["ui_settings"]["theme"]["preset"], "CatppuccinLatte");
        assert_eq!(json["notification_settings"]["bell_enabled"], false);
        assert!(json.to_string().contains("CatppuccinLatte"));
        assert!(!json.to_string().contains("/Users/"));

        let _ = fs::remove_dir_all(path.parent().expect("parent should exist"));
    }

    #[test]
    fn theme_onboarding_stays_off_for_existing_theme_shapes() {
        for raw in [
            r##"{"ui_settings":{"theme":{"overrides":{"accent":"#abc"}}}}"##,
            r#"{"ui_settings":{"theme_preset":"TerminalNative"}}"#,
        ] {
            let path = test_path();
            let store = Store::new_at(path.clone());
            fs::create_dir_all(path.parent().expect("parent should exist"))
                .expect("config dir should exist");
            fs::write(&path, raw).expect("config write should succeed");
            assert!(
                !store
                    .should_show_theme_onboarding()
                    .expect("configured theme should be readable"),
                "{raw}"
            );
            let _ = fs::remove_dir_all(path.parent().expect("parent should exist"));
        }
    }

    #[test]
    fn load_ui_settings_rejects_bad_theme_presets_with_actionable_copy() {
        let path = test_path();
        let store = Store::new_at(path.clone());
        fs::create_dir_all(path.parent().expect("parent should exist"))
            .expect("config dir should exist");

        fs::write(
            &path,
            r#"{
  "ui_settings": {
    "theme": {
      "preset": "catppuccin-neon"
    }
  }
}"#,
        )
        .expect("config write should succeed");

        let error = store.load_ui_settings().expect_err("load should fail");
        let message = error.to_string();
        assert!(
            message.contains("invalid muxboard theme preset `catppuccin-neon`"),
            "{message}"
        );
        assert!(
            message.contains("TerminalNative") && message.contains("RosePine"),
            "{message}"
        );
        assert!(
            message.contains("aliases include light, dark, system"),
            "{message}"
        );

        let _ = fs::remove_dir_all(path.parent().expect("parent should exist"));
    }

    #[test]
    fn config_example_matches_default_config_json() {
        let from_code = default_config_json().expect("default config should serialize");
        let from_code: Value = serde_json::from_str(&from_code).expect("default config is valid");

        let example_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("config.example.json");
        let from_file = fs::read_to_string(example_path).expect("config example should exist");
        let from_file: Value =
            serde_json::from_str(&from_file).expect("config example should be valid json");

        assert_eq!(from_file, from_code);
    }

    #[test]
    fn default_keybindings_json_matches_defaults_and_validates() {
        let json = default_keybindings_json().expect("default keybindings should serialize");
        let value: Value =
            serde_json::from_str(&json).expect("default keybindings should be valid json");
        let ui_settings: UiSettings =
            serde_json::from_value(value).expect("default keybindings should deserialize");

        ui_settings
            .validate()
            .expect("default keybindings should validate");
        assert_eq!(
            ui_settings.layout_preset,
            UiSettings::default().layout_preset
        );
        assert_eq!(ui_settings.theme, ThemeConfig::example());
        assert_eq!(
            ui_settings.active_theme_preset(),
            ThemePreset::TerminalNative
        );
        assert_eq!(ui_settings.keybindings, KeyBindingsConfig::default());
    }

    #[test]
    fn legacy_layout_preset_names_load_as_auto() {
        for legacy in ["Compact", "Standard", "Dense"] {
            let json = format!(
                r#"{{
  "layout_preset": "{legacy}",
  "theme_preset": "Calm",
  "keybindings": {{}}
}}"#
            );
            let ui_settings: UiSettings =
                serde_json::from_str(&json).expect("legacy layout preset should deserialize");

            assert_eq!(ui_settings.layout_preset, LayoutPreset::Auto);
            assert_eq!(ui_settings.keybindings, KeyBindingsConfig::default());
        }
    }

    #[test]
    fn load_ui_settings_rejects_conflicting_top_level_bindings() {
        for (bindings, expected) in [
            (
                r#""quit": ["q"],
      "jump": ["q"]"#,
                "conflicts with quit in the top-level scope",
            ),
            (
                r#""jump": ["g"],
      "refresh": ["g"]"#,
                "conflicts with jump in the top-level scope",
            ),
        ] {
            let path = test_path();
            let store = Store::new_at(path.clone());
            fs::create_dir_all(path.parent().expect("parent should exist"))
                .expect("config dir should exist");

            fs::write(
                &path,
                format!(
                    r#"{{
  "ui_settings": {{
    "keybindings": {{
      {bindings}
    }}
  }}
}}"#
                ),
            )
            .expect("config write should succeed");

            let error = store.load_ui_settings().expect_err("load should fail");
            let message = error.to_string();
            assert!(message.contains(expected), "{message}");

            let _ = fs::remove_dir_all(path.parent().expect("parent should exist"));
        }
    }

    #[test]
    fn load_ui_settings_rejects_unsupported_binding_tokens() {
        let path = test_path();
        let store = Store::new_at(path.clone());
        fs::create_dir_all(path.parent().expect("parent should exist"))
            .expect("config dir should exist");

        fs::write(
            &path,
            r#"{
  "ui_settings": {
    "keybindings": {
      "quit": ["pagedown"]
    }
  }
}"#,
        )
        .expect("config write should succeed");

        let error = store.load_ui_settings().expect_err("load should fail");
        assert!(error.to_string().contains("unsupported token `pagedown`"));

        let _ = fs::remove_dir_all(path.parent().expect("parent should exist"));
    }

    #[test]
    fn load_ui_settings_rejects_empty_binding_lists() {
        let path = test_path();
        let store = Store::new_at(path.clone());
        fs::create_dir_all(path.parent().expect("parent should exist"))
            .expect("config dir should exist");

        fs::write(
            &path,
            r#"{
  "ui_settings": {
    "keybindings": {
      "quit": []
    }
  }
}"#,
        )
        .expect("config write should succeed");

        let error = store.load_ui_settings().expect_err("load should fail");
        assert!(
            error
                .to_string()
                .contains("ui_settings.keybindings.quit must not be empty")
        );

        let _ = fs::remove_dir_all(path.parent().expect("parent should exist"));
    }

    #[test]
    fn load_ui_settings_rejects_padded_binding_tokens() {
        let path = test_path();
        let store = Store::new_at(path.clone());
        fs::create_dir_all(path.parent().expect("parent should exist"))
            .expect("config dir should exist");

        fs::write(
            &path,
            r#"{
  "ui_settings": {
    "keybindings": {
      "quit": [" q"]
    }
  }
}"#,
        )
        .expect("config write should succeed");

        let error = store.load_ui_settings().expect_err("load should fail");
        assert!(
            error
                .to_string()
                .contains("ui_settings.keybindings.quit contains an empty or padded token")
        );

        let _ = fs::remove_dir_all(path.parent().expect("parent should exist"));
    }

    #[test]
    fn load_ui_settings_rejects_conflicting_action_menu_bindings() {
        let path = test_path();
        let store = Store::new_at(path.clone());
        fs::create_dir_all(path.parent().expect("parent should exist"))
            .expect("config dir should exist");

        fs::write(
            &path,
            r#"{
  "ui_settings": {
    "keybindings": {
      "action_ack_selected": ["c"],
      "action_ack_clear_selected": ["c"]
    }
  }
}"#,
        )
        .expect("config write should succeed");

        let error = store.load_ui_settings().expect_err("load should fail");
        assert!(
            error
                .to_string()
                .contains("conflicts with action_ack_selected in the actions menu scope")
        );

        let _ = fs::remove_dir_all(path.parent().expect("parent should exist"));
    }
}
