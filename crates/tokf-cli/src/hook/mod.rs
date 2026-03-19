pub mod aider;
pub mod cline;
pub mod codex;
pub mod copilot;
pub mod cursor;
pub mod gemini;
pub mod instructions;
pub mod opencode;
pub mod permissions;
pub mod types;
pub mod windsurf;

use std::io::Read;
use std::path::{Path, PathBuf};

use permissions::PermissionVerdict;
use types::{CursorHookResponse, CursorInput, GeminiHookResponse, HookInput, HookResponse};

use crate::rewrite;
use crate::rewrite::types::RewriteConfig;
use crate::runner;

/// Permission decision behavior for the `handle` subcommand.
///
/// When multiple `PreToolUse` hooks are registered, the default `allow` mode
/// can block subsequent hooks from running. Use `preserve` to pass through
/// the original permission decision (or omit it) so other hooks can validate.
#[derive(clap::ValueEnum, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PermissionMode {
    /// Always return `permissionDecision: "allow"` (default, backward compatible)
    #[default]
    Allow,
    /// Omit `permissionDecision` from response, letting other hooks decide
    Preserve,
}

/// Process a `PreToolUse` hook invocation.
///
/// Reads JSON from stdin, checks if it's a Bash tool call, rewrites the command
/// if a matching rule is found, and prints the response JSON to stdout.
///
/// Returns `Ok(true)` if a rewrite was emitted, `Ok(false)` for pass-through.
/// Errors are intentionally swallowed to never block commands.
pub fn handle() -> bool {
    handle_with_permission(PermissionMode::Allow)
}

/// Process a `PreToolUse` hook invocation with permission mode control.
///
/// - `PermissionMode::Allow`: Always returns `permissionDecision: "allow"` (default, backward compatible)
/// - `PermissionMode::Preserve`: Omits `permissionDecision` so other hooks can validate
pub fn handle_with_permission(permission: PermissionMode) -> bool {
    let mut input = String::new();
    if std::io::stdin().read_to_string(&mut input).is_err() {
        return false;
    }
    handle_json_with_permission(&input, permission)
}

/// Core handle logic operating on a JSON string (convenience for tests).
#[cfg(test)]
pub(crate) fn handle_json(json: &str) -> bool {
    handle_json_with_permission(json, PermissionMode::Allow)
}

/// Core handle logic with permission mode control.
pub(crate) fn handle_json_with_permission(json: &str, permission: PermissionMode) -> bool {
    autodiscovery_with_permission(
        json,
        "Bash",
        permission,
        HookResponse::rewrite,
        HookResponse::rewrite_ask,
    )
}

/// Fully injectable handle logic with explicit permission rules (hermetic).
#[cfg(test)]
pub(crate) fn handle_json_with_rules(
    json: &str,
    user_config: &RewriteConfig,
    search_dirs: &[PathBuf],
    deny_rules: &[String],
    ask_rules: &[String],
) -> bool {
    handle_generic(
        json,
        "Bash",
        user_config,
        search_dirs,
        deny_rules,
        ask_rules,
        HookResponse::rewrite,
        HookResponse::rewrite_ask,
    )
}

/// Process a Gemini CLI `BeforeTool` hook invocation.
pub fn handle_gemini() -> bool {
    handle_gemini_with_permission(PermissionMode::Allow)
}

/// Process Gemini hook with permission mode control.
pub fn handle_gemini_with_permission(permission: PermissionMode) -> bool {
    let mut input = String::new();
    if std::io::stdin().read_to_string(&mut input).is_err() {
        return false;
    }
    handle_gemini_json_with_permission(&input, permission)
}

/// Core Gemini handle logic operating on a JSON string (convenience for tests).
#[cfg(test)]
pub(crate) fn handle_gemini_json(json: &str) -> bool {
    handle_gemini_json_with_permission(json, PermissionMode::Allow)
}

/// Core Gemini handle logic with permission mode control.
pub(crate) fn handle_gemini_json_with_permission(json: &str, permission: PermissionMode) -> bool {
    autodiscovery_with_permission(
        json,
        "run_shell_command",
        permission,
        GeminiHookResponse::rewrite,
        GeminiHookResponse::rewrite_ask,
    )
}

/// Fully injectable Gemini handle logic with explicit permission rules (hermetic).
#[cfg(test)]
pub(crate) fn handle_gemini_json_with_rules(
    json: &str,
    user_config: &RewriteConfig,
    search_dirs: &[PathBuf],
    deny_rules: &[String],
    ask_rules: &[String],
) -> bool {
    handle_generic(
        json,
        "run_shell_command",
        user_config,
        search_dirs,
        deny_rules,
        ask_rules,
        GeminiHookResponse::rewrite,
        GeminiHookResponse::rewrite_ask,
    )
}

/// Convenience wrapper that loads user config, search dirs, and permission
/// rules from the filesystem, then delegates to `handle_generic`.
/// Autodiscovery wrapper that selects response builders based on permission mode.
///
/// When `Preserve`, `build_ask_fn` is used for both allowed and ask responses
/// so that `permissionDecision` is omitted from the output.
fn autodiscovery_with_permission<R: serde::Serialize>(
    json: &str,
    expected_tool: &str,
    permission: PermissionMode,
    build_allow_fn: fn(String) -> R,
    build_ask_fn: fn(String) -> R,
) -> bool {
    match permission {
        PermissionMode::Allow => {
            handle_with_autodiscovery(json, expected_tool, build_allow_fn, build_ask_fn)
        }
        PermissionMode::Preserve => {
            handle_with_autodiscovery(json, expected_tool, build_ask_fn, build_ask_fn)
        }
    }
}

fn handle_with_autodiscovery<R: serde::Serialize>(
    json: &str,
    expected_tool: &str,
    build_allow: impl FnOnce(String) -> R,
    build_ask: impl FnOnce(String) -> R,
) -> bool {
    let user_config = rewrite::load_user_config().unwrap_or_default();
    let search_dirs = crate::config::default_search_dirs();
    let (deny, ask) = permissions::load_deny_ask_rules();
    handle_generic(
        json,
        expected_tool,
        &user_config,
        &search_dirs,
        &deny,
        &ask,
        build_allow,
        build_ask,
    )
}

/// Process a Cursor `preToolUse` hook invocation.
pub fn handle_cursor() -> bool {
    handle_cursor_with_permission(PermissionMode::Allow)
}

/// Process Cursor hook with permission mode control.
pub fn handle_cursor_with_permission(permission: PermissionMode) -> bool {
    let mut input = String::new();
    if std::io::stdin().read_to_string(&mut input).is_err() {
        return false;
    }
    handle_cursor_json_with_permission(&input, permission)
}

/// Core Cursor handle logic operating on a JSON string (convenience for tests).
///
/// Cursor's `beforeShellExecution` sends `command` at the top level
/// (not nested under `tool_input` like Claude Code / Gemini).
#[cfg(test)]
pub(crate) fn handle_cursor_json(json: &str) -> bool {
    handle_cursor_json_with_permission(json, PermissionMode::Allow)
}

/// Core Cursor handle logic with permission mode control.
pub(crate) fn handle_cursor_json_with_permission(json: &str, permission: PermissionMode) -> bool {
    let user_config = rewrite::load_user_config().unwrap_or_default();
    let search_dirs = crate::config::default_search_dirs();
    let (deny, ask) = permissions::load_deny_ask_rules();
    handle_cursor_json_inner(json, &user_config, &search_dirs, &deny, &ask, permission)
}

/// Fully injectable Cursor handle logic with explicit permission rules (hermetic).
#[cfg(test)]
pub(crate) fn handle_cursor_json_with_rules(
    json: &str,
    user_config: &RewriteConfig,
    search_dirs: &[PathBuf],
    deny_rules: &[String],
    ask_rules: &[String],
) -> bool {
    handle_cursor_json_inner(
        json,
        user_config,
        search_dirs,
        deny_rules,
        ask_rules,
        PermissionMode::Allow,
    )
}

/// Cursor handle logic with explicit permission rules and permission mode.
#[allow(clippy::too_many_arguments)]
fn handle_cursor_json_inner(
    json: &str,
    user_config: &RewriteConfig,
    search_dirs: &[PathBuf],
    deny_rules: &[String],
    ask_rules: &[String],
    permission: PermissionMode,
) -> bool {
    let Ok(input) = serde_json::from_str::<CursorInput>(json) else {
        return false;
    };

    let Some(command) = input.command else {
        return false;
    };

    let rewritten = rewrite::rewrite_with_config(&command, user_config, search_dirs, false);

    if rewritten == command {
        return false;
    }

    let response = match permission {
        PermissionMode::Allow => {
            match permissions::check_command_with_rules(&command, deny_rules, ask_rules) {
                PermissionVerdict::Deny => return false,
                PermissionVerdict::Ask => CursorHookResponse::rewrite_ask(rewritten),
                PermissionVerdict::Allow => CursorHookResponse::rewrite(rewritten),
            }
        }
        PermissionMode::Preserve => CursorHookResponse::rewrite_ask(rewritten),
    };

    if let Ok(json) = serde_json::to_string(&response) {
        println!("{json}");
        return true;
    }

    false
}

/// Generic handle logic shared across all hook formats.
///
/// Deserializes the JSON as the appropriate input type (inferred from the
/// response builder), checks the tool name, rewrites if a filter matches,
/// checks the original command against deny/ask permission rules, and
/// serializes the response to stdout.
///
/// - Deny rule matched → pass through (return false), letting the tool's
///   native deny handling block the command.
/// - Ask rule matched → rewrite but omit `permissionDecision` so the tool
///   prompts the user for confirmation.
/// - No rule matched → rewrite and auto-allow (existing behavior).
#[allow(clippy::too_many_arguments)]
fn handle_generic<R: serde::Serialize>(
    json: &str,
    expected_tool: &str,
    user_config: &RewriteConfig,
    search_dirs: &[PathBuf],
    deny_rules: &[String],
    ask_rules: &[String],
    build_allow: impl FnOnce(String) -> R,
    build_ask: impl FnOnce(String) -> R,
) -> bool {
    // All three input types share the same JSON shape (tool_name + tool_input.command),
    // so we can deserialize once with the Claude Code type.
    let Ok(hook_input) = serde_json::from_str::<HookInput>(json) else {
        return false;
    };

    if hook_input.tool_name != expected_tool {
        return false;
    }

    let Some(command) = hook_input.tool_input.command else {
        return false;
    };

    let rewritten = rewrite::rewrite_with_config(&command, user_config, search_dirs, false);

    if rewritten == command {
        return false;
    }

    // Check the ORIGINAL command against deny/ask rules before responding.
    let response = match permissions::check_command_with_rules(&command, deny_rules, ask_rules) {
        PermissionVerdict::Deny => return false,
        PermissionVerdict::Ask => build_ask(rewritten),
        PermissionVerdict::Allow => build_allow(rewritten),
    };

    if let Ok(json) = serde_json::to_string(&response) {
        println!("{json}");
        return true;
    }

    false
}

/// Install the hook shim and register it in Claude Code settings.
///
/// # Errors
///
/// Returns an error if file I/O fails.
pub fn install(global: bool, tokf_bin: &str, install_context: bool) -> anyhow::Result<()> {
    let (hook_dir, settings_path) = if global {
        let user = crate::paths::user_dir()
            .ok_or_else(|| anyhow::anyhow!("could not determine config directory"))?;
        let hook_dir = user.join("hooks");
        let home = dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("could not determine home directory"))?;
        let settings_path = home.join(".claude/settings.json");
        (hook_dir, settings_path)
    } else {
        let cwd = std::env::current_dir()?;
        let hook_dir = cwd.join(".tokf/hooks");
        let settings_path = cwd.join(".claude/settings.json");
        (hook_dir, settings_path)
    };

    install_to(&hook_dir, &settings_path, tokf_bin, install_context)
}

/// Core install logic with explicit paths (testable).
pub(crate) fn install_to(
    hook_dir: &Path,
    settings_path: &Path,
    tokf_bin: &str,
    install_context: bool,
) -> anyhow::Result<()> {
    let hook_script = hook_dir.join("pre-tool-use.sh");
    write_hook_shim(hook_dir, &hook_script, tokf_bin, "--permission preserve")?;
    patch_json_hook_config(settings_path, &hook_script, "PreToolUse", "Bash", None)?;

    eprintln!("[tokf] hook installed");
    eprintln!("[tokf]   script: {}", hook_script.display());
    eprintln!("[tokf]   settings: {}", settings_path.display());

    if install_context && let Some(claude_dir) = settings_path.parent() {
        let created = write_context_doc(claude_dir)?;
        patch_md_with_reference(claude_dir, "CLAUDE.md")?;
        if created {
            eprintln!("[tokf]   context: {}", claude_dir.join("TOKF.md").display());
        } else {
            eprintln!(
                "[tokf]   context: {} (already exists, skipped)",
                claude_dir.join("TOKF.md").display()
            );
        }
    }

    Ok(())
}

/// Resolve hook dir and tool-specific paths for global or project-local installation.
///
/// Returns `(hook_dir, tool_config_dir)` where:
/// - `hook_dir`: where the shim script goes (e.g. `~/.tokf/hooks` or `.tokf/hooks`)
/// - `tool_config_dir`: tool-specific directory (e.g. `~/.gemini` or `.gemini`)
pub(crate) fn resolve_paths(
    global: bool,
    tool_dir_name: &str,
) -> anyhow::Result<(PathBuf, PathBuf)> {
    if global {
        let user = crate::paths::user_dir()
            .ok_or_else(|| anyhow::anyhow!("could not determine config directory"))?;
        let hook_dir = user.join("hooks");
        let home = dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("could not determine home directory"))?;
        let tool_dir = home.join(tool_dir_name);
        Ok((hook_dir, tool_dir))
    } else {
        let cwd = std::env::current_dir()?;
        let hook_dir = cwd.join(".tokf/hooks");
        let tool_dir = cwd.join(tool_dir_name);
        Ok((hook_dir, tool_dir))
    }
}

/// Write the TOKF.md context file that explains the compression indicator.
/// Skips writing if the file already exists (preserves user edits).
/// Returns `true` if the file was created, `false` if it already existed.
pub(crate) fn write_context_doc(dir: &Path) -> anyhow::Result<bool> {
    std::fs::create_dir_all(dir)?;
    let tokf_md = dir.join("TOKF.md");
    if tokf_md.exists() {
        return Ok(false);
    }
    let content = "\
🗜️ means this output was compressed by tokf.
Run `tokf raw last` to see the full uncompressed output of the last command.
";
    std::fs::write(&tokf_md, content)?;
    Ok(true)
}

/// Add an `@TOKF.md` reference to an md file (creates the file if needed).
///
/// Used for `CLAUDE.md`, `GEMINI.md`, etc.
pub(crate) fn patch_md_with_reference(dir: &Path, filename: &str) -> anyhow::Result<()> {
    let md_path = dir.join(filename);
    let marker = "@TOKF.md";
    match std::fs::read_to_string(&md_path) {
        Ok(content) if content.contains(marker) => Ok(()),
        Ok(content) => {
            let separator = if content.is_empty() || content.ends_with('\n') {
                ""
            } else {
                "\n"
            };
            let updated = format!("{content}{separator}{marker}\n");
            std::fs::write(&md_path, updated)?;
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            std::fs::write(&md_path, format!("{marker}\n"))?;
            Ok(())
        }
        Err(e) => Err(e.into()),
    }
}

/// Write the hook shim script. `extra_args` is appended after `hook handle`
/// (e.g. `"--format gemini"`). A space is inserted automatically if non-empty.
pub(crate) fn write_hook_shim(
    hook_dir: &Path,
    hook_script: &Path,
    tokf_bin: &str,
    extra_args: &str,
) -> anyhow::Result<()> {
    std::fs::create_dir_all(hook_dir)?;

    let escaped_bin = if tokf_bin == "tokf" {
        tokf_bin.to_string()
    } else {
        runner::shell_escape(tokf_bin)
    };
    let suffix = if extra_args.is_empty() {
        String::new()
    } else {
        format!(" {}", extra_args.trim())
    };
    let content = format!("#!/bin/sh\nexec {escaped_bin} hook handle{suffix}\n");
    std::fs::write(hook_script, content)?;

    // Make executable on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o755);
        std::fs::set_permissions(hook_script, perms)?;
    }

    Ok(())
}

/// Patch a JSON settings/config file to register a tokf hook entry.
///
/// Works for both Claude Code `settings.json` and Gemini `settings.json`.
/// For Cursor, which uses a different structure, see `cursor::patch_hooks_json`.
///
/// - `hook_event_key`: e.g. `"PreToolUse"` or `"BeforeTool"`
/// - `matcher`: e.g. `"Bash"` or `"run_shell_command"`
/// - `initial_value`: optional initial JSON object (e.g. for Cursor's `"version": 1`)
pub(crate) fn patch_json_hook_config(
    settings_path: &Path,
    hook_script: &Path,
    hook_event_key: &str,
    matcher: &str,
    initial_value: Option<serde_json::Value>,
) -> anyhow::Result<()> {
    let mut settings: serde_json::Value = if settings_path.exists() {
        let content = std::fs::read_to_string(settings_path)?;
        serde_json::from_str(&content).map_err(|e| {
            anyhow::anyhow!("corrupt settings.json at {}: {e}", settings_path.display())
        })?
    } else {
        initial_value.unwrap_or_else(|| serde_json::json!({}))
    };

    let hook_command = runner::shell_escape(
        hook_script
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("hook script path is not valid UTF-8"))?,
    );

    let tokf_hook_entry = serde_json::json!({
        "matcher": matcher,
        "hooks": [{ "type": "command", "command": hook_command }]
    });

    let hooks = settings
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("settings.json is not an object"))?
        .entry("hooks")
        .or_insert_with(|| serde_json::json!({}));

    let hook_array = hooks
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("settings.json hooks is not an object"))?
        .entry(hook_event_key)
        .or_insert_with(|| serde_json::json!([]));

    let arr = hook_array
        .as_array_mut()
        .ok_or_else(|| anyhow::anyhow!("hooks.{hook_event_key} is not an array"))?;

    // Remove any existing tokf hook entries (idempotent install)
    arr.retain(|entry| {
        let is_tokf = entry
            .get("hooks")
            .and_then(|h| h.as_array())
            .is_some_and(|hooks| {
                hooks.iter().any(|h| {
                    h.get("command")
                        .and_then(serde_json::Value::as_str)
                        .is_some_and(|cmd| cmd.contains("tokf") && cmd.contains("hook"))
                })
            });
        !is_tokf
    });

    arr.push(tokf_hook_entry);

    // Write atomically: write to temp file then rename
    if let Some(parent) = settings_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(&settings)?;
    let tmp_path = settings_path.with_extension("json.tmp");
    std::fs::write(&tmp_path, &json)?;
    std::fs::rename(&tmp_path, settings_path)?;

    Ok(())
}

/// Append or replace a tokf section in a markdown file, idempotent via markers.
pub(crate) fn append_or_replace_section(
    path: &Path,
    content_fn: impl FnOnce() -> String,
) -> anyhow::Result<()> {
    let start_marker = "<!-- tokf:start -->";
    let end_marker = "<!-- tokf:end -->";

    let existing = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e.into()),
    };

    let start_pos = existing.find(start_marker);
    let end_pos = existing.find(end_marker);

    // Only replace when both markers are present and in correct order.
    // If only the start marker exists (missing end), fall through to append
    // to avoid truncating user content after the start marker.
    if let (Some(s), Some(e)) = (start_pos, end_pos)
        && s < e
    {
        let before = &existing[..s];
        let after = &existing[e + end_marker.len()..];
        let section = content_fn();
        let updated = format!("{before}{section}{after}");
        std::fs::write(path, updated)?;
        return Ok(());
    }

    // No valid marker pair found — append the section.
    let separator = if existing.is_empty() || existing.ends_with('\n') {
        ""
    } else {
        "\n"
    };
    let section = content_fn();
    let updated = format!("{existing}{separator}\n{section}");
    std::fs::write(path, updated)?;

    Ok(())
}

/// Write an instruction/convention file (creates parent dirs, overwrites).
pub(crate) fn write_instruction_file(path: &Path, content: &str) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, content)?;
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests;
