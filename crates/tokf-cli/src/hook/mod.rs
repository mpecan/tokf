pub mod aider;
pub mod cline;
pub mod codex;
pub mod copilot;
pub mod cursor;
mod debug_log;
pub mod gemini;
mod install;
pub mod instructions;
pub mod opencode;
pub mod permission_engine;
pub mod permissions;
pub mod types;
pub mod windsurf;

use std::io::Read;
use std::path::PathBuf;

#[cfg(test)]
use install::install_to;
use install::{
    append_or_replace_section, patch_json_hook_config, patch_json_hook_config_with_command,
    patch_md_with_reference, resolve_paths, write_context_doc, write_hook_shim,
    write_instruction_file,
};
use permission_engine::ErrorFallback;
use permissions::PermissionVerdict;
use tokf_hook_types::PermissionDecision;
use types::{
    CodexHookResponse, CursorHookResponse, CursorInput, GeminiHookResponse, HookFormat, HookInput,
    HookResponse,
};

use crate::rewrite;
use crate::rewrite::types::{PermissionEngineType, RewriteConfig};

use crate::runtime::Runtime;

/// Install the hook shim and register it in Claude Code settings.
///
/// # Errors
///
/// Returns an error if file I/O fails.
pub fn install(
    rt: &Runtime,
    global: bool,
    tokf_bin: &str,
    install_context: bool,
) -> anyhow::Result<()> {
    install::install(rt, global, tokf_bin, install_context)
}

/// Outcome of a hook handle invocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookOutcome {
    /// A response was emitted with an allow decision (or auto-allow rewrite).
    Allow,
    /// A response was emitted with an ask decision.
    Ask,
    /// A response was emitted with a deny decision.
    Deny,
    /// No response was emitted — pass-through.
    PassThrough,
}

/// Codex rewrite protocol selected for the installed hook shim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexRewriteMode {
    /// Codex 0.131.0+ supports transparent `updatedInput` rewrites.
    UpdatedInput,
    /// Older Codex builds parse but ignore `updatedInput`, so deny with a rerun hint.
    DenyRerun,
}

impl CodexRewriteMode {
    pub const fn env_value(self) -> &'static str {
        match self {
            Self::UpdatedInput => "updated-input",
            Self::DenyRerun => "deny-rerun",
        }
    }

    fn from_runtime(rt: &Runtime) -> Self {
        match rt.codex_rewrite_mode() {
            Some(value) if value == Self::UpdatedInput.env_value() => Self::UpdatedInput,
            _ => Self::DenyRerun,
        }
    }
}

/// Process a `PreToolUse` hook invocation.
///
/// Reads JSON from stdin, checks if it's a Bash tool call, rewrites the command
/// if a matching rule is found, and prints the response JSON to stdout.
///
/// Returns the outcome of the hook (allow/ask/deny/pass-through).
/// Errors are intentionally swallowed to never block commands.
pub fn handle(rt: &Runtime, no_cache: bool) -> HookOutcome {
    handle_from_reader_with_cache(rt, &mut std::io::stdin(), no_cache)
}

fn handle_from_reader_with_cache<R: Read>(
    rt: &Runtime,
    reader: &mut R,
    no_cache: bool,
) -> HookOutcome {
    let mut input = String::new();
    if reader.read_to_string(&mut input).is_err() {
        return HookOutcome::PassThrough;
    }

    handle_json_with_cache(rt, &input, no_cache)
}

/// Core handle logic operating on a JSON string.
#[cfg(test)]
pub(crate) fn handle_json(json: &str) -> HookOutcome {
    let rt = &Runtime::isolated();
    handle_json_with_cache(rt, json, false)
}

fn handle_json_with_cache(rt: &Runtime, json: &str, no_cache: bool) -> HookOutcome {
    handle_with_autodiscovery(
        rt,
        json,
        "Bash",
        HookFormat::ClaudeCode,
        no_cache,
        HookResponse::rewrite,
        HookResponse::rewrite_ask,
        HookResponse::deny,
        HookOutcome::Allow,
        HookOutcome::Ask,
    )
}

/// Fully injectable handle logic with explicit config (hermetic).
#[cfg(test)]
pub(crate) fn handle_json_with_rules(
    json: &str,
    user_config: &RewriteConfig,
    search_dirs: &[PathBuf],
) -> HookOutcome {
    let rt = &Runtime::isolated();
    handle_generic(
        rt,
        json,
        "Bash",
        HookFormat::ClaudeCode,
        user_config,
        search_dirs,
        false,
        HookResponse::rewrite,
        HookResponse::rewrite_ask,
        HookResponse::deny,
        HookOutcome::Allow,
        HookOutcome::Ask,
    )
}

/// Process a Gemini CLI `BeforeTool` hook invocation.
pub fn handle_gemini(rt: &Runtime, no_cache: bool) -> HookOutcome {
    let mut input = String::new();
    if std::io::stdin().read_to_string(&mut input).is_err() {
        return HookOutcome::PassThrough;
    }
    handle_gemini_json_with_cache(rt, &input, no_cache)
}

/// Core Gemini handle logic operating on a JSON string.
#[cfg(test)]
pub(crate) fn handle_gemini_json(json: &str) -> HookOutcome {
    let rt = &Runtime::isolated();
    handle_gemini_json_with_cache(rt, json, false)
}

fn handle_gemini_json_with_cache(rt: &Runtime, json: &str, no_cache: bool) -> HookOutcome {
    handle_with_autodiscovery(
        rt,
        json,
        "run_shell_command",
        HookFormat::Gemini,
        no_cache,
        GeminiHookResponse::rewrite,
        GeminiHookResponse::rewrite_ask,
        GeminiHookResponse::deny,
        HookOutcome::Allow,
        HookOutcome::Ask,
    )
}

/// Fully injectable Gemini handle logic with explicit config (hermetic).
#[cfg(test)]
pub(crate) fn handle_gemini_json_with_rules(
    json: &str,
    user_config: &RewriteConfig,
    search_dirs: &[PathBuf],
) -> HookOutcome {
    let rt = &Runtime::isolated();
    handle_generic(
        rt,
        json,
        "run_shell_command",
        HookFormat::Gemini,
        user_config,
        search_dirs,
        false,
        GeminiHookResponse::rewrite,
        GeminiHookResponse::rewrite_ask,
        GeminiHookResponse::deny,
        HookOutcome::Allow,
        HookOutcome::Ask,
    )
}

/// Convenience wrapper that loads user config and search dirs from the
/// filesystem, then delegates to `handle_generic`.
#[allow(clippy::too_many_arguments)]
fn handle_with_autodiscovery<R: serde::Serialize>(
    rt: &Runtime,
    json: &str,
    expected_tool: &str,
    format: HookFormat,
    no_cache: bool,
    build_allow: impl FnOnce(String, Option<String>) -> R,
    build_ask: impl FnOnce(String, Option<String>) -> R,
    build_deny: impl FnOnce(String, Option<String>) -> R,
    allow_outcome: HookOutcome,
    ask_outcome: HookOutcome,
) -> HookOutcome {
    let user_config = rewrite::load_user_config(rt).unwrap_or_default();
    let search_dirs = crate::config::default_search_dirs(rt);
    handle_generic(
        rt,
        json,
        expected_tool,
        format,
        &user_config,
        &search_dirs,
        no_cache,
        build_allow,
        build_ask,
        build_deny,
        allow_outcome,
        ask_outcome,
    )
}

/// Process a Cursor `preToolUse` hook invocation.
pub fn handle_cursor(rt: &Runtime, no_cache: bool) -> HookOutcome {
    let mut input = String::new();
    if std::io::stdin().read_to_string(&mut input).is_err() {
        return HookOutcome::PassThrough;
    }
    handle_cursor_json_with_cache(rt, &input, no_cache)
}

/// Core Cursor handle logic operating on a JSON string.
///
/// Cursor's `beforeShellExecution` sends `command` at the top level
/// (not nested under `tool_input` like Claude Code / Gemini).
#[cfg(test)]
pub(crate) fn handle_cursor_json(json: &str) -> HookOutcome {
    let rt = &Runtime::isolated();
    handle_cursor_json_with_cache(rt, json, false)
}

fn handle_cursor_json_with_cache(rt: &Runtime, json: &str, no_cache: bool) -> HookOutcome {
    let user_config = rewrite::load_user_config(rt).unwrap_or_default();
    let search_dirs = crate::config::default_search_dirs(rt);
    handle_cursor_json_inner(rt, json, &user_config, &search_dirs, no_cache)
}

/// Fully injectable Cursor handle logic with explicit config (hermetic).
#[cfg(test)]
pub(crate) fn handle_cursor_json_with_rules(
    json: &str,
    user_config: &RewriteConfig,
    search_dirs: &[PathBuf],
) -> HookOutcome {
    let rt = &Runtime::isolated();
    handle_cursor_json_inner(rt, json, user_config, search_dirs, false)
}

/// Process an `OpenAI` Codex CLI `PreToolUse` hook invocation.
pub fn handle_codex(rt: &Runtime, no_cache: bool) -> HookOutcome {
    let mut input = String::new();
    if std::io::stdin().read_to_string(&mut input).is_err() {
        return HookOutcome::PassThrough;
    }
    handle_codex_json_with_cache(rt, &input, no_cache)
}

/// Core Codex handle logic operating on a JSON string.
#[cfg(test)]
pub(crate) fn handle_codex_json(json: &str) -> HookOutcome {
    let rt = &Runtime::isolated();
    handle_codex_json_with_cache(rt, json, false)
}

fn handle_codex_json_with_cache(rt: &Runtime, json: &str, no_cache: bool) -> HookOutcome {
    handle_codex_json_with_mode(rt, json, no_cache, CodexRewriteMode::from_runtime(rt))
}

fn handle_codex_json_with_mode(
    rt: &Runtime,
    json: &str,
    no_cache: bool,
    mode: CodexRewriteMode,
) -> HookOutcome {
    match mode {
        CodexRewriteMode::UpdatedInput => handle_with_autodiscovery(
            rt,
            json,
            "Bash",
            HookFormat::Codex,
            no_cache,
            CodexHookResponse::rewrite,
            CodexHookResponse::rewrite_ask,
            CodexHookResponse::deny,
            HookOutcome::Allow,
            HookOutcome::Deny,
        ),
        CodexRewriteMode::DenyRerun => handle_with_autodiscovery(
            rt,
            json,
            "Bash",
            HookFormat::Codex,
            no_cache,
            CodexHookResponse::rewrite_deny_rerun,
            CodexHookResponse::rewrite_ask,
            CodexHookResponse::deny,
            HookOutcome::Deny,
            HookOutcome::Deny,
        ),
    }
}

/// Cursor handle logic with explicit permission rules.
fn handle_cursor_json_inner(
    rt: &Runtime,
    json: &str,
    user_config: &RewriteConfig,
    search_dirs: &[PathBuf],
    no_cache: bool,
) -> HookOutcome {
    let Ok(input) = serde_json::from_str::<CursorInput>(json) else {
        return HookOutcome::PassThrough;
    };

    let Some(command) = input.command else {
        return HookOutcome::PassThrough;
    };

    process_command(
        rt,
        &command,
        "shell",
        json,
        HookFormat::Cursor,
        user_config,
        search_dirs,
        no_cache,
        CursorHookResponse::rewrite,
        CursorHookResponse::rewrite_ask,
        CursorHookResponse::deny,
        HookOutcome::Allow,
        HookOutcome::Ask,
    )
}

/// Query the external permission engine for a verdict.
///
/// Only called when an external engine is configured. On engine failure,
/// applies the `on_error` fallback (ask/allow/builtin).
fn query_external_engine(
    cmd: &str,
    hook_json: &str,
    format: HookFormat,
    user_config: &RewriteConfig,
) -> Option<PermissionVerdict> {
    let ext_config = user_config
        .permissions
        .as_ref()
        .filter(|p| p.engine == PermissionEngineType::External)?
        .external
        .as_ref()?;

    let verdict = match permission_engine::check_with_engine(ext_config, hook_json, format) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("[tokf] warning: external permission engine failed: {e}");
            match ext_config.on_error {
                ErrorFallback::Ask => PermissionVerdict::ask(None),
                ErrorFallback::Allow => PermissionVerdict::allow(),
                ErrorFallback::Builtin => {
                    let (deny, ask) = permissions::load_deny_ask_rules();
                    permissions::check_command_with_rules(cmd, &deny, &ask)
                }
            }
        }
    };
    Some(verdict)
}

/// Generic handle logic shared across all hook formats.
///
/// When an external permission engine is configured, it is consulted on every
/// command (even unfiltered ones). Otherwise, tokf only acts when a filter
/// matches — the AI tool handles its own permissions natively.
#[allow(clippy::too_many_arguments)]
fn handle_generic<R: serde::Serialize>(
    rt: &Runtime,
    json: &str,
    expected_tool: &str,
    format: HookFormat,
    user_config: &RewriteConfig,
    search_dirs: &[PathBuf],
    no_cache: bool,
    build_allow: impl FnOnce(String, Option<String>) -> R,
    build_ask: impl FnOnce(String, Option<String>) -> R,
    build_deny: impl FnOnce(String, Option<String>) -> R,
    allow_outcome: HookOutcome,
    ask_outcome: HookOutcome,
) -> HookOutcome {
    let Ok(hook_input) = serde_json::from_str::<HookInput>(json) else {
        return HookOutcome::PassThrough;
    };

    if hook_input.tool_name != expected_tool {
        return HookOutcome::PassThrough;
    }

    let Some(command) = hook_input.tool_input.command else {
        return HookOutcome::PassThrough;
    };

    process_command(
        rt,
        &command,
        expected_tool,
        json,
        format,
        user_config,
        search_dirs,
        no_cache,
        build_allow,
        build_ask,
        build_deny,
        allow_outcome,
        ask_outcome,
    )
}

/// Shared post-deserialization logic for all hook formats.
///
/// Handles both the external-engine path (verdict on every command) and
/// the no-engine path (rewrite-only, host-specific response). When
/// `TOKF_HOOK_LOG` is set in the env, every invocation appends one
/// diagnostic record.
#[allow(clippy::too_many_arguments)]
fn process_command<R: serde::Serialize>(
    rt: &Runtime,
    command: &str,
    tool_name: &str,
    json: &str,
    format: HookFormat,
    user_config: &RewriteConfig,
    search_dirs: &[PathBuf],
    no_cache: bool,
    build_allow: impl FnOnce(String, Option<String>) -> R,
    build_ask: impl FnOnce(String, Option<String>) -> R,
    build_deny: impl FnOnce(String, Option<String>) -> R,
    allow_outcome: HookOutcome,
    ask_outcome: HookOutcome,
) -> HookOutcome {
    let (outcome, after) = decide(
        rt,
        command,
        json,
        format,
        user_config,
        search_dirs,
        no_cache,
        build_allow,
        build_ask,
        build_deny,
        allow_outcome,
        ask_outcome,
    );
    debug_log::log_event(tool_name, format, command, after.as_deref(), outcome);
    outcome
}

/// Compute the hook decision and emit the response for it. Returns the
/// outcome plus the rewritten command string when one was emitted (for
/// the diagnostic log; `None` means no rewrite was sent to the agent).
#[allow(clippy::too_many_arguments)]
fn decide<R: serde::Serialize>(
    rt: &Runtime,
    command: &str,
    json: &str,
    format: HookFormat,
    user_config: &RewriteConfig,
    search_dirs: &[PathBuf],
    no_cache: bool,
    build_allow: impl FnOnce(String, Option<String>) -> R,
    build_ask: impl FnOnce(String, Option<String>) -> R,
    build_deny: impl FnOnce(String, Option<String>) -> R,
    allow_outcome: HookOutcome,
    ask_outcome: HookOutcome,
) -> (HookOutcome, Option<String>) {
    // When an external permission engine is configured, consult it on every
    // command — even ones tokf has no filter for.
    if let Some(verdict) = query_external_engine(command, json, format, user_config) {
        // Deny doesn't need a rewrite — the command won't execute.
        if verdict.decision == PermissionDecision::Deny {
            if emit_response(&build_deny(command.to_string(), verdict.reason)) {
                return (HookOutcome::Deny, None);
            }
            return (HookOutcome::PassThrough, None);
        }
        let rewritten = rewrite::rewrite_with_config(
            rewrite::RewriteCtx {
                rt,
                user_config,
                search_dirs,
            },
            command,
            no_cache,
        );
        let rewrite_changed = rewritten != command;
        let output_cmd = if rewrite_changed {
            rewritten
        } else {
            command.to_string()
        };
        let logged_after = rewrite_changed.then(|| output_cmd.clone());
        let (response, outcome) = match verdict.decision {
            PermissionDecision::Ask if format == HookFormat::Codex && !rewrite_changed => (
                build_deny(command.to_string(), verdict.reason),
                HookOutcome::Deny,
            ),
            PermissionDecision::Ask => (build_ask(output_cmd, verdict.reason), ask_outcome),
            _ if format == HookFormat::Codex && !rewrite_changed => {
                return (HookOutcome::PassThrough, None);
            }
            _ => (build_allow(output_cmd, verdict.reason), allow_outcome),
        };
        if emit_response(&response) {
            return (outcome, logged_after);
        }
        return (HookOutcome::PassThrough, logged_after);
    }

    // No external engine — only act when tokf has a matching filter.
    let rewritten = rewrite::rewrite_with_config(
        rewrite::RewriteCtx {
            rt,
            user_config,
            search_dirs,
        },
        command,
        no_cache,
    );
    if rewritten == command {
        return (HookOutcome::PassThrough, None);
    }

    let logged_after = Some(rewritten.clone());
    if emit_response(&build_allow(rewritten, None)) {
        (allow_outcome, logged_after)
    } else {
        (HookOutcome::PassThrough, logged_after)
    }
}

/// Serialize and print a hook response. Returns true on success.
fn emit_response<R: serde::Serialize>(response: &R) -> bool {
    if let Ok(json) = serde_json::to_string(response) {
        println!("{json}");
        return true;
    }
    false
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests;
