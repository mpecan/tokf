pub mod detect;

use std::io::IsTerminal;

use crate::history::{load_project_config, save_project_config};
use crate::runtime::Runtime;

/// Section stored in `config.toml` under `[setup]`.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct TokfSetupSection {
    pub completed: Option<bool>,
    pub completed_at: Option<String>,
}

/// Returns `true` if a previous `tokf setup` run completed successfully.
pub fn is_setup_completed(rt: &Runtime) -> bool {
    let Some(path) = rt.global_config_path() else {
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
pub fn mark_setup_completed(rt: &Runtime) -> anyhow::Result<()> {
    let path = rt.require_global_config_path()?;
    let mut config = load_project_config(&path);
    config.setup = Some(TokfSetupSection {
        completed: Some(true),
        completed_at: Some(crate::sync_core::utc_now_iso8601()),
    });
    save_project_config(&path, &config)
}

/// Print a setup hint to stderr if stdin is a TTY and setup hasn't been completed.
pub fn hint_setup_if_needed(rt: &Runtime) {
    if !std::io::stderr().is_terminal() {
        return;
    }
    if is_setup_completed(rt) {
        return;
    }
    eprintln!("[tokf] Tip: run `tokf setup` to auto-detect your AI tools and install hooks.");
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn is_setup_completed_returns_false_when_no_config() {
        let rt = Runtime::isolated();
        assert!(!is_setup_completed(&rt));
    }

    #[test]
    fn mark_and_check_setup_completed() {
        let rt = Runtime::isolated();
        assert!(!is_setup_completed(&rt));
        mark_setup_completed(&rt).unwrap();
        assert!(is_setup_completed(&rt));
    }
}
