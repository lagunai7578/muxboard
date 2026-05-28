use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::{
    app::{AttentionKey, TargetGroup},
    core::PaneStatus,
    paths,
};

const MAX_PERSISTED_COMMANDS: usize = 8;
const MACRO_SLOT_COUNT: usize = 5;

#[derive(Debug, Clone)]
pub struct Store {
    path: PathBuf,
}

impl Store {
    pub fn new() -> Result<Self> {
        Ok(Self {
            path: paths::state_file()?,
        })
    }

    #[cfg(test)]
    pub(crate) fn new_at(path: PathBuf) -> Self {
        Self { path }
    }

    pub(crate) fn load_acknowledged_attention(&self) -> Result<HashMap<AttentionKey, PaneStatus>> {
        Ok(self
            .load_state()?
            .acknowledged_attention
            .into_iter()
            .map(|entry| (entry.key, entry.status))
            .collect())
    }

    pub(crate) fn save_acknowledged_attention(
        &self,
        acknowledged_attention: &HashMap<AttentionKey, PaneStatus>,
    ) -> Result<()> {
        let mut state = self.load_state()?;
        let mut entries = acknowledged_attention
            .iter()
            .map(|(key, status)| PersistedAcknowledgement {
                key: key.clone(),
                status: *status,
            })
            .collect::<Vec<_>>();

        entries.sort_by(|left, right| left.key.sort_key().cmp(&right.key.sort_key()));

        state.acknowledged_attention = entries;
        self.save_state(&state)
    }

    pub(crate) fn load_command_state(&self) -> Result<(Vec<String>, Vec<Option<String>>)> {
        let state = self.load_state()?;
        Ok((state.recent_commands, state.macro_slots))
    }

    pub(crate) fn save_command_state(
        &self,
        recent_commands: &[String],
        macro_slots: &[Option<String>],
    ) -> Result<()> {
        let mut state = self.load_state()?;
        state.recent_commands = recent_commands
            .iter()
            .take(MAX_PERSISTED_COMMANDS)
            .cloned()
            .collect();
        state.macro_slots = normalized_macro_slots(macro_slots);
        self.save_state(&state)
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn load_target_groups(&self) -> Result<Vec<TargetGroup>> {
        Ok(self.load_state()?.target_groups)
    }

    pub(crate) fn save_target_groups(&self, target_groups: &[TargetGroup]) -> Result<()> {
        let mut state = self.load_state()?;
        let mut groups = target_groups.to_vec();
        groups.sort_by(|left, right| left.name.cmp(&right.name));
        state.target_groups = groups;
        self.save_state(&state)
    }

    fn load_state(&self) -> Result<PersistedState> {
        if !self.path.exists() {
            return Ok(PersistedState::default());
        }

        let raw = fs::read_to_string(&self.path)
            .with_context(|| format!("failed to read {}", self.path.display()))?;
        serde_json::from_str(&raw).context("failed to parse persisted muxboard state")
    }

    fn save_state(&self, state: &PersistedState) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }

        let json = serde_json::to_string_pretty(state).context("failed to serialize state")?;
        atomic_write(&self.path, &json)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct PersistedState {
    acknowledged_attention: Vec<PersistedAcknowledgement>,
    recent_commands: Vec<String>,
    macro_slots: Vec<Option<String>>,
    target_groups: Vec<TargetGroup>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedAcknowledgement {
    key: AttentionKey,
    status: PaneStatus,
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

fn normalized_macro_slots(macro_slots: &[Option<String>]) -> Vec<Option<String>> {
    let mut slots = macro_slots
        .iter()
        .take(MACRO_SLOT_COUNT)
        .cloned()
        .collect::<Vec<_>>();
    slots.resize(MACRO_SLOT_COUNT, None);
    slots
}

#[cfg(test)]
mod tests {
    use super::Store;
    use crate::{
        app::{AttentionKey, PaneLocator, TargetGroup},
        core::PaneStatus,
    };
    use std::{
        collections::HashMap,
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
                "muxboard-state-test-{}-{}-{unique}",
                std::process::id(),
                TEST_PATH_COUNTER.fetch_add(1, Ordering::Relaxed)
            ))
            .join("state.json")
    }

    fn blocked_parent_path(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should work")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "muxboard-state-parent-blocked-{label}-{}-{}-{unique}",
            std::process::id(),
            TEST_PATH_COUNTER.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn missing_state_file_loads_empty_defaults_and_exposes_path() {
        let path = test_path();
        let store = Store::new_at(path.clone());

        assert_eq!(store.path(), path.as_path());
        assert!(
            store
                .load_acknowledged_attention()
                .expect("missing state should load")
                .is_empty()
        );
        assert_eq!(
            store
                .load_command_state()
                .expect("missing command state should load"),
            (Vec::new(), Vec::new())
        );
        assert!(
            store
                .load_target_groups()
                .expect("missing target groups should load")
                .is_empty()
        );
    }

    #[test]
    fn malformed_state_file_returns_a_parse_error() {
        let path = test_path();
        fs::create_dir_all(path.parent().expect("parent should exist"))
            .expect("state dir should exist");
        fs::write(&path, "{ definitely not json").expect("state write should succeed");
        let store = Store::new_at(path.clone());

        let error = store
            .load_command_state()
            .expect_err("malformed state should fail");
        assert!(
            error
                .to_string()
                .contains("failed to parse persisted muxboard state")
        );

        let _ = fs::remove_dir_all(path.parent().expect("parent should exist"));
    }

    #[test]
    fn store_round_trips_acknowledgements() {
        let path = test_path();
        let store = Store::new_at(path.clone());
        let mut acknowledged = HashMap::new();
        acknowledged.insert(
            AttentionKey {
                session_name: String::from("demo"),
                window_name: String::from("ops"),
                pane_index: 1,
                current_path: String::from("/tmp"),
                current_command: String::from("codex"),
                title: String::from("agent"),
            },
            PaneStatus::Waiting,
        );

        store
            .save_acknowledged_attention(&acknowledged)
            .expect("save should succeed");
        let loaded = store
            .load_acknowledged_attention()
            .expect("load should succeed");

        assert_eq!(loaded, acknowledged);
        let _ = fs::remove_dir_all(path.parent().expect("parent should exist"));
    }

    #[test]
    fn save_acknowledgements_reports_unusable_state_parent() {
        let blocked = blocked_parent_path("acknowledgements");
        fs::write(&blocked, "not a directory").expect("blocking file should exist");
        let store = Store::new_at(blocked.join("state.json"));

        let error = store
            .save_acknowledged_attention(&HashMap::new())
            .expect_err("blocked state parent should fail");

        assert!(error.to_string().contains("failed to create"));
        let _ = fs::remove_file(blocked);
    }

    #[test]
    fn store_round_trips_command_state() {
        let path = test_path();
        let store = Store::new_at(path.clone());
        let recent = vec![String::from("cargo test"), String::from("git status")];
        let macros = vec![
            Some(String::from("continue")),
            None,
            Some(String::from("cargo check")),
            None,
            Some(String::from("npm test")),
        ];

        store
            .save_command_state(&recent, &macros)
            .expect("save should succeed");
        let (loaded_recent, loaded_macros) =
            store.load_command_state().expect("load should succeed");

        assert_eq!(loaded_recent, recent);
        assert_eq!(loaded_macros, macros);
        let _ = fs::remove_dir_all(path.parent().expect("parent should exist"));
    }

    #[test]
    fn command_state_limits_recent_commands_and_normalizes_macro_slots() {
        let path = test_path();
        let store = Store::new_at(path.clone());
        let recent = (0..12)
            .map(|index| format!("command {index}"))
            .collect::<Vec<_>>();

        store
            .save_command_state(
                &recent,
                &[
                    Some(String::from("one")),
                    Some(String::from("two")),
                    Some(String::from("three")),
                    Some(String::from("four")),
                    Some(String::from("five")),
                    Some(String::from("six")),
                ],
            )
            .expect("save should succeed");
        let (loaded_recent, loaded_macros) =
            store.load_command_state().expect("load should succeed");

        assert_eq!(loaded_recent, recent[..8]);
        assert_eq!(
            loaded_macros,
            vec![
                Some(String::from("one")),
                Some(String::from("two")),
                Some(String::from("three")),
                Some(String::from("four")),
                Some(String::from("five")),
            ]
        );

        store
            .save_command_state(&[], &[Some(String::from("only"))])
            .expect("save should succeed");
        let (_, padded_macros) = store.load_command_state().expect("load should succeed");
        assert_eq!(
            padded_macros,
            vec![Some(String::from("only")), None, None, None, None]
        );

        let _ = fs::remove_dir_all(path.parent().expect("parent should exist"));
    }

    #[test]
    fn store_round_trips_target_groups() {
        let path = test_path();
        let store = Store::new_at(path.clone());
        let members = vec![
            PaneLocator {
                session_name: String::from("demo"),
                window_name: String::from("ops"),
                pane_index: 0,
            },
            PaneLocator {
                session_name: String::from("demo"),
                window_name: String::from("ops"),
                pane_index: 1,
            },
        ];
        let groups = vec![
            TargetGroup {
                name: String::from("triage"),
                members: members.clone(),
            },
            TargetGroup {
                name: String::from("all"),
                members,
            },
        ];

        store
            .save_target_groups(&groups)
            .expect("save should succeed");
        let loaded = store.load_target_groups().expect("load should succeed");

        assert_eq!(loaded, vec![groups[1].clone(), groups[0].clone()]);
        let _ = fs::remove_dir_all(path.parent().expect("parent should exist"));
    }
}
