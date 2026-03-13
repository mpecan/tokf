---
title: Integrations
description: Connect tokf to Claude Code, Gemini CLI, Cursor, Cline, Windsurf, Copilot, Aider, OpenCode, and Codex.
order: 7
---

## Claude Code hook

tokf integrates with [Claude Code](https://claude.ai/code) as a `PreToolUse` hook that **automatically filters every `Bash` tool call** — no changes to your workflow required.

```sh
tokf hook install          # project-local (.tokf/)
tokf hook install --global # user-level (~/.config/tokf/)
```

Once installed, every command Claude runs through the Bash tool is filtered transparently. Track cumulative savings with `tokf gain`.

### Custom binary path

By default the generated hook script calls bare `tokf`, relying on PATH at runtime. If `tokf` isn't on PATH in the hook's execution environment (common with Linuxbrew or `cargo install` when PATH is only set in interactive shell profiles), pass `--path` to embed a specific binary location:

```sh
tokf hook install --global --path ~/.cargo/bin/tokf
tokf hook install --tool opencode --path /home/linuxbrew/.linuxbrew/bin/tokf
```

tokf also ships a filter-authoring skill that teaches Claude the complete filter schema:

```sh
tokf skill install          # project-local (.claude/skills/)
tokf skill install --global # user-level (~/.claude/skills/)
```

## Gemini CLI

tokf integrates with [Gemini CLI](https://github.com/google-gemini/gemini-cli) as a `BeforeTool` hook that automatically filters `run_shell_command` tool calls.

```sh
tokf hook install --tool gemini-cli          # project-local (.gemini/)
tokf hook install --tool gemini-cli --global # user-level (~/.gemini/)
```

This registers a hook shim in `.gemini/settings.json` (or `~/.gemini/settings.json` for `--global`). When `--no-context` is not set, it also creates `.gemini/TOKF.md` and patches `.gemini/GEMINI.md` with context about the compression indicator.

## Cursor

tokf integrates with [Cursor](https://cursor.com) via a `preToolUse` hook that automatically filters `Shell` tool calls.

```sh
tokf hook install --tool cursor          # project-local (.cursor/)
tokf hook install --tool cursor --global # user-level (~/.cursor/)
```

This registers a hook in `.cursor/hooks.json` (or `~/.cursor/hooks.json` for `--global`). When `--no-context` is not set, it also creates `.cursor/rules/tokf.md` with context about the compression indicator.

## Cline

tokf integrates with [Cline](https://cline.bot) via a rules file that instructs the agent to prefix supported commands with `tokf run`.

```sh
tokf hook install --tool cline          # project-local (.clinerules/)
tokf hook install --tool cline --global # user-level (~/Documents/Cline/Rules/)
```

This writes `.clinerules/tokf.md` (or `~/Documents/Cline/Rules/tokf.md` for `--global`), which Cline auto-discovers. The rules file uses `alwaysApply: true` frontmatter.

## Windsurf

tokf integrates with [Windsurf](https://windsurf.com) via a rules file.

```sh
tokf hook install --tool windsurf          # project-local (.windsurf/rules/)
tokf hook install --tool windsurf --global # user-level (appends to global rules)
```

Project-local creates `.windsurf/rules/tokf.md`. Global mode appends a tokf section (with `<!-- tokf:start/end -->` markers for idempotent updates) to `~/.codeium/windsurf/memories/global_rules.md`.

## GitHub Copilot

tokf integrates with [GitHub Copilot](https://github.com/features/copilot) via instruction files. Copilot only supports repo-level instructions (no `--global` option).

```sh
tokf hook install --tool copilot
```

This creates `.github/instructions/tokf.instructions.md` (with `applyTo: "**"` frontmatter) and appends a tokf section to `.github/copilot-instructions.md`.

## Aider

tokf integrates with [Aider](https://aider.chat) via conventions files.

```sh
tokf hook install --tool aider          # project-local (CONVENTIONS.md)
tokf hook install --tool aider --global # user-level (patches ~/.aider.conf.yml)
```

Project-local appends a tokf section to `CONVENTIONS.md` (which Aider auto-discovers). Global mode writes a conventions file and adds it to `~/.aider.conf.yml`'s `read:` list.

## OpenCode

tokf integrates with [OpenCode](https://opencode.ai) via a plugin that applies filters in real-time before command execution.

**Requirements:** OpenCode with Bun runtime installed.

**Install (project-local):**
```sh
tokf hook install --tool opencode
```

**Install (global):**
```sh
tokf hook install --tool opencode --global
```

This writes `.opencode/plugins/tokf.ts` (or `~/.config/opencode/plugins/tokf.ts` for `--global`), which OpenCode auto-loads. The plugin uses OpenCode's `tool.execute.before` hook to intercept `bash` tool calls and rewrites the command in-place when a matching filter exists. **Restart OpenCode after installation for the plugin to take effect.**

If tokf rewrite fails or no filter matches, the command passes through unmodified (fail-safe).

## OpenAI Codex CLI

tokf integrates with [OpenAI Codex CLI](https://github.com/openai/codex) via a skill that instructs the agent to prefix supported commands with `tokf run`.

**Install (project-local):**
```sh
tokf hook install --tool codex
```

**Install (global):**
```sh
tokf hook install --tool codex --global
```

This writes `.agents/skills/tokf-run/SKILL.md` (or `~/.agents/skills/tokf-run/SKILL.md` for `--global`), which Codex auto-discovers. Unlike the Claude Code hook (which intercepts commands at the tool level), the Codex integration is skill-based: it teaches the agent to use `tokf run` as a command prefix. If tokf is not installed, the agent falls back to running commands without the prefix (fail-safe).

## Creating Filters with Claude

tokf ships a Claude Code skill that teaches Claude the complete filter schema, processing order, step types, template pipes, and naming conventions.

**Invoke automatically**: Claude will activate the skill whenever you ask to create or modify a filter — just describe what you want in natural language:

> "Create a filter for `npm install` output that keeps only warnings and errors"
> "Write a tokf filter for `pytest` that shows a summary on success and failure details on fail"

**Invoke explicitly** with the `/tokf-filter` slash command:

```
/tokf-filter create a filter for docker build output
```

The skill is in `.claude/skills/tokf-filter/SKILL.md`. Reference material (exhaustive step docs and an annotated example TOML) lives in `.claude/skills/tokf-filter/references/`.

## Task runners

tokf also integrates with task runners like `make` and `just` by injecting itself as the task runner's shell. Each recipe line is individually filtered while exit codes propagate correctly. See [Rewrite configuration](#rewrite-configuration-rewritestoml) for details.
