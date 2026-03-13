---
name: tokf-discover
description: Find missed token savings in Claude Code sessions and create filters for unfiltered commands
user_invocable: true
---

# tokf discover — Find Missed Token Savings

Use this skill to analyze Claude Code sessions and find commands that are running without tokf filtering, wasting tokens on verbose output.

## Quick Start

Run `tokf discover` in the project directory to scan recent sessions:

```bash
tokf discover
```

## Options

- `--all` — scan all projects, not just the current one
- `--since 7d` — only scan sessions from the last 7 days (also `24h`, `30m`)
- `--limit 0` — show all results (default: top 20)
- `--json` — output as JSON for programmatic use
- `--session <path>` — scan a specific session file
- `--project <path>` — scan sessions for a specific project path

## Interpreting Results

The output shows:
- **COMMAND** — the shell command pattern being run without filtering
- **FILTER** — the tokf filter that would handle it
- **RUNS** — how many times it appeared in sessions
- **TOKENS** — estimated token count of unfiltered output
- **SAVINGS** — estimated tokens that filtering would save

## Workflow

1. Run `tokf discover` to identify top savings opportunities
2. For commands with existing filters: run `tokf hook install` to set up automatic filtering
3. For commands without filters: use `/tokf-filter` skill to create a custom filter
4. Re-run `tokf discover` after changes to verify improvement

## Creating Filters for Unfiltered Commands

If `tokf discover` shows commands with no matching filter, create one:

```bash
# See what a filter would look like
tokf which "the-command --args"

# Use the tokf-filter skill to create a proper filter
# /tokf-filter
```

## JSON Output

Use `--json` for integration with other tools:

```bash
tokf discover --json | jq '.results[] | select(.estimated_savings > 1000)'
```

The JSON schema includes:
- `sessions_scanned` — number of JSONL files processed
- `total_commands` — all Bash commands found
- `already_filtered` — commands already using tokf
- `filterable_commands` — commands with available filters
- `no_filter_commands` — commands with no matching filter
- `estimated_total_savings` — total estimated token savings
- `results[]` — per-command breakdown sorted by savings
