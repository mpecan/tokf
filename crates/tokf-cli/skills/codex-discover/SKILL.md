---
name: tokf-discover
description: Find missed token savings by scanning AI coding session files for commands that ran without tokf filtering.
---

# tokf discover — Find Missed Token Savings

Use this skill to analyze past sessions and find commands that are running without tokf filtering, wasting tokens on verbose output.

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

## Workflow

1. Run `tokf discover` to identify top savings opportunities
2. For commands with existing filters: run `tokf hook install --tool codex` to set up automatic filtering
3. For commands without filters: create a custom filter with `tokf eject`
4. Re-run `tokf discover` after changes to verify improvement

## JSON Output

Use `--json` for integration with other tools:

```bash
tokf discover --json
```
