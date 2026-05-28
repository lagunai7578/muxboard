use std::{
    env,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};

const APP_NAME: &str = "muxboard";

pub fn config_file() -> Result<PathBuf> {
    Ok(config_dir()?.join("config.json"))
}

pub fn state_file() -> Result<PathBuf> {
    Ok(state_dir()?.join("state.json"))
}

pub fn config_dir() -> Result<PathBuf> {
    xdg_dir("XDG_CONFIG_HOME", [".config"])
}

pub fn state_dir() -> Result<PathBuf> {
    xdg_dir("XDG_STATE_HOME", [".local", "state"])
}

pub fn cache_dir() -> Result<PathBuf> {
    xdg_dir("XDG_CACHE_HOME", [".cache"])
}

pub fn data_dir() -> Result<PathBuf> {
    xdg_dir("XDG_DATA_HOME", [".local", "share"])
}

#[cfg(test)]
pub(crate) fn app_name() -> &'static str {
    APP_NAME
}

fn xdg_dir<const N: usize>(var: &str, fallback_parts: [&str; N]) -> Result<PathBuf> {
    xdg_dir_from(env::var_os(var), env::var_os("HOME"), fallback_parts)
}

fn xdg_dir_from<const N: usize>(
    configured: Option<std::ffi::OsString>,
    home: Option<std::ffi::OsString>,
    fallback_parts: [&str; N],
) -> Result<PathBuf> {
    if let Some(path) = configured {
        return Ok(PathBuf::from(path).join(APP_NAME));
    }

    let home = home_dir_from(home)?;
    Ok(join_parts(home, fallback_parts).join(APP_NAME))
}

fn home_dir_from(home: Option<std::ffi::OsString>) -> Result<PathBuf> {
    home.map(PathBuf::from).context("HOME is not set")
}

fn join_parts<const N: usize>(base: PathBuf, parts: [&str; N]) -> PathBuf {
    parts.iter().fold(base, |path, part| path.join(part))
}

#[allow(dead_code)]
fn _assert_path_ref(_: &Path) {}

#[cfg(test)]
mod tests {
    use super::{
        app_name, cache_dir, config_dir, config_file, data_dir, home_dir_from, join_parts,
        state_dir, state_file, xdg_dir_from,
    };
    use crate::{config, state};
    use std::env;
    use std::ffi::OsString;
    use std::path::PathBuf;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    struct EnvGuard {
        saved: Vec<(&'static str, Option<OsString>)>,
    }

    impl EnvGuard {
        fn set(vars: &[(&'static str, Option<&str>)]) -> Self {
            let saved = vars
                .iter()
                .map(|(name, _)| (*name, env::var_os(name)))
                .collect::<Vec<_>>();

            for (name, value) in vars {
                // SAFETY: These path tests serialize all environment mutation through ENV_LOCK.
                unsafe {
                    match value {
                        Some(value) => env::set_var(name, value),
                        None => env::remove_var(name),
                    }
                }
            }

            Self { saved }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for (name, value) in &self.saved {
                // SAFETY: These path tests serialize all environment mutation through ENV_LOCK.
                unsafe {
                    match value {
                        Some(value) => env::set_var(name, value),
                        None => env::remove_var(name),
                    }
                }
            }
        }
    }

    #[test]
    fn app_name_is_muxboard() {
        assert_eq!(app_name(), "muxboard");
    }

    #[test]
    fn join_parts_builds_expected_path() {
        let path = join_parts(PathBuf::from("/tmp/home"), [".config", "nested"]);
        assert_eq!(path, PathBuf::from("/tmp/home/.config/nested"));
    }

    #[test]
    fn xdg_dir_prefers_configured_xdg_location() {
        let path = xdg_dir_from(
            Some(OsString::from("/tmp/xdg")),
            Some(OsString::from("/tmp/home")),
            [".config"],
        )
        .expect("configured xdg dir should work");

        assert_eq!(path, PathBuf::from("/tmp/xdg/muxboard"));
    }

    #[test]
    fn xdg_dir_falls_back_to_home_parts() {
        let path = xdg_dir_from(None, Some(OsString::from("/tmp/home")), [".local", "state"])
            .expect("home fallback should work");

        assert_eq!(path, PathBuf::from("/tmp/home/.local/state/muxboard"));
    }

    #[test]
    fn xdg_dir_reports_missing_home_when_no_xdg_override_exists() {
        let error = xdg_dir_from(None, None, [".cache"]).expect_err("missing home should fail");

        assert!(error.to_string().contains("HOME is not set"));
    }

    #[test]
    fn home_dir_from_reports_missing_home() {
        let error = home_dir_from(None).expect_err("missing home should fail");

        assert!(error.to_string().contains("HOME is not set"));
    }

    #[test]
    fn public_path_helpers_append_muxboard_to_xdg_roots() {
        let _lock = ENV_LOCK.lock().expect("env lock should not be poisoned");
        let _env = EnvGuard::set(&[
            ("XDG_CONFIG_HOME", Some("/tmp/muxboard-xdg/config")),
            ("XDG_STATE_HOME", Some("/tmp/muxboard-xdg/state")),
            ("XDG_CACHE_HOME", Some("/tmp/muxboard-xdg/cache")),
            ("XDG_DATA_HOME", Some("/tmp/muxboard-xdg/data")),
            ("HOME", None),
        ]);

        assert_eq!(
            config_dir().expect("config dir should resolve"),
            PathBuf::from("/tmp/muxboard-xdg/config/muxboard")
        );
        assert_eq!(
            config_file().expect("config file should resolve"),
            PathBuf::from("/tmp/muxboard-xdg/config/muxboard/config.json")
        );
        assert_eq!(
            state_dir().expect("state dir should resolve"),
            PathBuf::from("/tmp/muxboard-xdg/state/muxboard")
        );
        assert_eq!(
            state_file().expect("state file should resolve"),
            PathBuf::from("/tmp/muxboard-xdg/state/muxboard/state.json")
        );
        assert_eq!(
            cache_dir().expect("cache dir should resolve"),
            PathBuf::from("/tmp/muxboard-xdg/cache/muxboard")
        );
        assert_eq!(
            data_dir().expect("data dir should resolve"),
            PathBuf::from("/tmp/muxboard-xdg/data/muxboard")
        );
    }

    #[test]
    fn config_and_state_stores_use_xdg_style_files() {
        let _lock = ENV_LOCK.lock().expect("env lock should not be poisoned");
        let _env = EnvGuard::set(&[
            ("XDG_CONFIG_HOME", Some("/tmp/muxboard-store-xdg/config")),
            ("XDG_STATE_HOME", Some("/tmp/muxboard-store-xdg/state")),
            ("HOME", None),
        ]);

        assert_eq!(
            config::Store::new()
                .expect("config store should resolve from XDG")
                .path(),
            PathBuf::from("/tmp/muxboard-store-xdg/config/muxboard/config.json")
        );
        assert_eq!(
            state::Store::new()
                .expect("state store should resolve from XDG")
                .path(),
            PathBuf::from("/tmp/muxboard-store-xdg/state/muxboard/state.json")
        );
    }

    #[test]
    fn public_path_helpers_fall_back_to_home_when_xdg_is_absent() {
        let _lock = ENV_LOCK.lock().expect("env lock should not be poisoned");
        let _env = EnvGuard::set(&[
            ("XDG_CONFIG_HOME", None),
            ("XDG_STATE_HOME", None),
            ("XDG_CACHE_HOME", None),
            ("XDG_DATA_HOME", None),
            ("HOME", Some("/tmp/muxboard-home")),
        ]);

        assert_eq!(
            config_file().expect("config file should resolve"),
            PathBuf::from("/tmp/muxboard-home/.config/muxboard/config.json")
        );
        assert_eq!(
            state_file().expect("state file should resolve"),
            PathBuf::from("/tmp/muxboard-home/.local/state/muxboard/state.json")
        );
        assert_eq!(
            cache_dir().expect("cache dir should resolve"),
            PathBuf::from("/tmp/muxboard-home/.cache/muxboard")
        );
        assert_eq!(
            data_dir().expect("data dir should resolve"),
            PathBuf::from("/tmp/muxboard-home/.local/share/muxboard")
        );
    }
}
