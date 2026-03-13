---
title: Discover Missed Savings
description: Scan Claude Code sessions to find commands running without tokf filtering and estimate token waste.
order: 10
---

## tokf discover

`tokf discover` scans Claude Code session files to find commands that ran without tokf filtering, estimates the token waste, and ranks results by savings opportunity.

```bash
# Scan sessions for the current project
tokf discover

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

COMMAND                        FILTER               RUNS     TOKENS    SAVINGS
--------------------------------------------------------------------------------
cargo test                     cargo/test             89      45.2k      27.1k
git diff                       git/diff               67      31.0k      18.6k
cargo clippy                   cargo/clippy           45      22.1k      13.3k

Estimated total savings: 58.9k tokens (201 filterable, 443 with no filter)
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

### How It Works

1. Locates Claude Code session JSONL files in `~/.claude/projects/`
2. Extracts all Bash `tool_use` / `tool_result` pairs
3. Skips commands already wrapped with `tokf run`
4. Matches remaining commands against available tokf filters
5. Uses historical compression ratios from your tracking database (falls back to 60% default)
6. Aggregates and ranks by estimated token savings
