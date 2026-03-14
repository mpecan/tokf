---
title: Discover Missed Savings
description: Scan Claude Code sessions to find commands running without tokf filtering and estimate token waste.
order: 10
---

## tokf discover

`tokf discover` scans Claude Code session files to find commands that have **no matching tokf filter**, helping you identify where to create new filters for maximum token savings.

By default, commands that already have a matching filter are hidden — if the hook is installed, those are already being filtered. Use `--include-filtered` to see the full picture including commands with existing filters.

```bash
# Scan sessions for the current project
tokf discover

# Also show commands that have existing filters
tokf discover --include-filtered

# Scan all projects
tokf discover --all

# Only sessions from the last 7 days
tokf discover --since 7d

# JSON output for programmatic use
tokf discover --json
```

Example output:

```
[tokf] scanned 12 sessions, 847 commands total
[tokf] 203 already filtered by tokf
[tokf] 201 commands have filters (use --include-filtered to show)

COMMAND                        FILTER               RUNS     TOKENS
----------------------------------------------------------------------
python manage.py migrate       (none)                 34      12.1k
terraform plan                 (none)                 28       9.8k
helm upgrade                   (none)                 15       6.2k

Total unfiltered output: 28.1k tokens across 443 commands
```

### Options

| Flag | Description |
|------|-------------|
| `--project <path>` | Scan sessions for a specific project path |
| `--all` | Scan sessions across all projects |
| `--session <path>` | Scan a single session JSONL file |
| `--since <duration>` | Filter by recency: `7d`, `24h`, `30m` |
| `--limit <n>` | Number of results to show (0 = all, default: 20) |
| `--json` | Output as JSON |
| `--include-filtered` | Also show commands that already have a matching filter |

### How It Works

1. Locates Claude Code session JSONL files in `~/.claude/projects/`
2. Extracts all Bash `tool_use` / `tool_result` pairs
3. Skips commands already wrapped with `tokf run`
4. Matches remaining commands against available tokf filters
5. By default shows only commands with no matching filter
6. Aggregates and ranks by estimated token count
