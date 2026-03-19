pub mod detect;

use std::io::IsTerminal;

use crate::history::{load_project_config, save_project_config};
use crate::paths::user_dir;

/// Section stored in `config.toml` under `[setup]`.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct TokfSetupSection {
    pub completed: Option<bool>,
    pub completed_at: Option<String>,
}

/// Returns `true` if a previous `tokf setup` run completed successfully.
pub fn is_setup_completed() -> bool {
    let Some(path) = user_dir().map(|d| d.join("config.toml")) else {
        return false;
    };
    let config = load_project_config(&path);
    config
        .setup
        .as_ref()
        .and_then(|s| s.completed)
        .unwrap_or(false)
}

/// Mark setup as completed in the global `config.toml`.
///
/// # Errors
/// Returns an error if the config directory cannot be determined or the file cannot be written.
pub fn mark_setup_completed() -> anyhow::Result<()> {
    let path = user_dir()
        .map(|d| d.join("config.toml"))
        .ok_or_else(|| anyhow::anyhow!("cannot determine config directory"))?;
    let mut config = load_project_config(&path);
    config.setup = Some(TokfSetupSection {
        completed: Some(true),
        completed_at: Some(crate::sync_core::utc_now_iso8601()),
    });
    save_project_config(&path, &config)
}

/// Print a setup hint to stderr if stdin is a TTY and setup hasn't been completed.
pub fn hint_setup_if_needed() {
    if !std::io::stderr().is_terminal() {
        return;
    }
    if is_setup_completed() {
        return;
    }
    eprintln!("[tokf] Tip: run `tokf setup` to auto-detect your AI tools and install hooks.");
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use serial_test::serial;

    use super::*;

    #[test]
    #[serial]
    fn is_setup_completed_returns_false_when_no_config() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = crate::paths::HomeGuard::set(dir.path());
        assert!(!is_setup_completed());
    }

    #[test]
    #[serial]
    fn mark_and_check_setup_completed() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = crate::paths::HomeGuard::set(dir.path());
        assert!(!is_setup_completed());
        mark_setup_completed().unwrap();
        assert!(is_setup_completed());
    }
}
