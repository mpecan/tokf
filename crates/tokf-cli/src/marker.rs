//! Rendering of the compression indicator and the per-entry recovery marker.
//!
//! tokf annotates filtered output; it never replaces it. The filtered body is
//! printed exactly as before — the marker is a short prefix on the indicator
//! that is already paid for.
//!
//! Forms:
//! - `""` — indicator disabled (`output.show_indicator = false`).
//! - `"🗜️ "` — compressed, but this run was not recorded in history.
//! - `"🗜️#42 "` — compressed and recoverable: `tokf raw 42`.
//!
//! `🗜️#<id>` extends the existing `🗜️` vocabulary rather than introducing a
//! second marker language. Measured against cl100k, the id costs ~3 tokens on
//! top of an indicator that is already printed — no extra line, no extra
//! newline. A denser encoding does not help: BPE packs digit runs (up to three
//! digits per token) while mixed-case alphanumerics fragment, so base36/base62
//! ids of the same value cost the same or more.
//!
//! Recovery is deliberately the CLI (`tokf raw <id>`) rather than a dedicated
//! tool call: shell output can be post-processed — piped through `grep`, `head`,
//! or tokf itself — whereas a tool result lands in context whole. Recovering a
//! large entry must not be able to blow the context window.

use tokf::history::OutputConfig;

use tokf::runtime::Runtime;

/// Render the prefix that precedes filtered output.
///
/// The id is attached whenever the run was recorded in history, which is the
/// only condition under which `tokf raw <id>` can resolve it. When the
/// indicator is disabled nothing at all is emitted — the id is not smuggled
/// back in.
pub fn render_indicator(show_indicator: bool, history_id: Option<i64>) -> String {
    if !show_indicator {
        return String::new();
    }
    history_id.map_or_else(|| "🗜️ ".to_string(), |id| format!("🗜️#{id} "))
}

/// Load the print-time config for the current working directory's project.
pub fn load_render_config(rt: &Runtime) -> OutputConfig {
    let cwd = std::env::current_dir().unwrap_or_default();
    let project_root = tokf::history::project_root_for(&cwd);
    OutputConfig::load(rt, Some(&project_root))
}

/// Print filtered output with the appropriate indicator/marker prefix.
///
/// Additive by construction: `body` is printed verbatim, whatever the prefix.
pub fn print_with_indicator(body: &str, cfg: &OutputConfig, history_id: Option<i64>) {
    let prefix = render_indicator(cfg.show_indicator, history_id);
    println!("{prefix}{body}");
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::render_indicator;

    #[test]
    fn indicator_disabled_emits_nothing() {
        assert_eq!(render_indicator(false, Some(42)), "");
        assert_eq!(render_indicator(false, None), "");
    }

    #[test]
    fn no_history_id_falls_back_to_plain_indicator() {
        assert_eq!(render_indicator(true, None), "🗜️ ");
    }

    #[test]
    fn history_id_emits_marker() {
        assert_eq!(render_indicator(true, Some(42)), "🗜️#42 ");
    }

    #[test]
    fn large_ids_format_without_panic() {
        let out = render_indicator(true, Some(i64::MAX));
        assert_eq!(out, format!("🗜️#{} ", i64::MAX));
    }
}
