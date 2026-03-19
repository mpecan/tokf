use std::io::IsTerminal;

use tokf::setup::detect::{DetectedTool, Tool, detect_all};
use tokf::setup::{is_setup_completed, mark_setup_completed};

use crate::commands::{HookTool, cmd_hook_install, cmd_skill_install};

const fn tool_to_hook(tool: Tool) -> HookTool {
    match tool {
        Tool::ClaudeCode => HookTool::ClaudeCode,
        Tool::GeminiCli => HookTool::GeminiCli,
        Tool::Cursor => HookTool::Cursor,
        Tool::Cline => HookTool::Cline,
        Tool::Windsurf => HookTool::Windsurf,
        Tool::Copilot => HookTool::Copilot,
        Tool::Aider => HookTool::Aider,
        Tool::OpenCode => HookTool::OpenCode,
        Tool::Codex => HookTool::Codex,
    }
}

pub fn cmd_setup(refresh: bool) -> i32 {
    if !std::io::stdin().is_terminal() {
        return cmd_setup_non_interactive();
    }

    if !refresh && is_setup_completed() {
        eprintln!("[tokf] Setup already completed. Use `tokf setup --refresh` to re-run.");
        return 0;
    }

    eprintln!("[tokf] Detecting installed AI tools...\n");
    let detected = detect_all();

    if detected.is_empty() {
        eprintln!("[tokf] No supported AI tools detected.");
        eprintln!("[tokf] Install an AI coding tool and run `tokf setup` again,");
        eprintln!("[tokf] or install hooks manually with `tokf hook install --tool <name>`.");
        return 0;
    }

    print_detected(&detected);

    let Some(selections) = prompt_tool_selection(&detected) else {
        eprintln!("[tokf] Setup cancelled.");
        return 0;
    };

    if selections.is_empty() {
        eprintln!("[tokf] No tools selected. Run `tokf setup` again when ready.");
        return 0;
    }

    let global = prompt_global();
    let install_skill = selections.iter().any(|&i| detected[i].supports_skill)
        && prompt_skill_install(&detected, &selections);

    let failed = run_installs(&detected, &selections, global, install_skill);
    print_summary(selections.len(), failed);

    i32::from(failed > 0)
}

fn run_installs(
    detected: &[DetectedTool],
    selections: &[usize],
    global: bool,
    install_skill: bool,
) -> usize {
    eprintln!();
    let mut failed = 0;
    for &idx in selections {
        let dt = &detected[idx];
        let hook_tool = tool_to_hook(dt.tool);
        eprintln!("[tokf] Installing hook for {}...", dt.display_name);
        if cmd_hook_install(global, &hook_tool, None, true) != 0 {
            failed += 1;
        }
    }

    if install_skill {
        eprintln!("[tokf] Installing Claude Code filter-authoring skill...");
        if cmd_skill_install(global) != 0 {
            eprintln!("[tokf] Warning: skill install failed.");
        }
    }

    if let Err(e) = mark_setup_completed() {
        eprintln!("[tokf] Warning: could not save setup state: {e:#}");
    }
    failed
}

fn print_summary(total: usize, failed: usize) {
    eprintln!();
    if failed == 0 {
        eprintln!(
            "[tokf] Done! {total} tool(s) configured. Every command your AI agent runs is now filtered.",
        );
    } else {
        eprintln!(
            "[tokf] Done with {failed} error(s). {} of {total} tool(s) installed successfully.",
            total - failed,
        );
    }
}

fn cmd_setup_non_interactive() -> i32 {
    eprintln!("[tokf] Detecting installed AI tools (non-interactive mode)...\n");
    let detected = detect_all();

    if detected.is_empty() {
        eprintln!("[tokf] No supported AI tools detected.");
        return 0;
    }

    print_detected(&detected);
    eprintln!("\n[tokf] Run these commands to install hooks:\n");
    for dt in &detected {
        let tool_flag = if dt.tool == Tool::ClaudeCode {
            String::new()
        } else {
            format!(" --tool {}", dt.tool.cli_value())
        };
        eprintln!("  tokf hook install --global{tool_flag}");
    }
    eprintln!();
    0
}

fn print_detected(detected: &[DetectedTool]) {
    for dt in detected {
        eprintln!("  [+] {} ({})", dt.display_name, dt.evidence);
    }
}

fn prompt_tool_selection(detected: &[DetectedTool]) -> Option<Vec<usize>> {
    eprintln!();
    let items: Vec<String> = detected
        .iter()
        .map(|d| d.display_name.to_string())
        .collect();
    let defaults: Vec<bool> = vec![true; items.len()];

    dialoguer::MultiSelect::new()
        .with_prompt("Which tools should tokf install hooks for?")
        .items(&items)
        .defaults(&defaults)
        .interact_opt()
        .ok()?
}

fn prompt_global() -> bool {
    let choices = [
        "Global (recommended — works in every project)",
        "Local (current project only)",
    ];
    dialoguer::Select::new()
        .with_prompt("Install scope")
        .items(choices)
        .default(0)
        .interact_opt()
        .ok()
        .flatten()
        .unwrap_or(0)
        == 0
}

fn prompt_skill_install(detected: &[DetectedTool], selections: &[usize]) -> bool {
    let has_claude = selections
        .iter()
        .any(|&i| detected[i].tool == Tool::ClaudeCode);
    if !has_claude {
        return false;
    }

    dialoguer::Confirm::new()
        .with_prompt("Install the Claude Code filter-authoring skill?")
        .default(true)
        .interact()
        .unwrap_or(true)
}
