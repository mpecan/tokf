---
title: Token Savings Tracking
description: See exactly how much context tokf saves you, run by run.
order: 4
---

tokf records input/output byte counts per run in a local SQLite database:

```sh
tokf gain              # summary: total bytes saved and reduction %
tokf gain --daily      # day-by-day breakdown
tokf gain --by-filter  # breakdown by filter
tokf gain --json       # machine-readable output
```

## Output history

tokf records raw and filtered outputs in a local SQLite database, useful for debugging filters or reviewing what an AI agent saw:

```sh
tokf history list              # recent entries (current project)
tokf history list -l 20        # show 20 entries
tokf history list --all        # entries from all projects
tokf history show 42           # full details for entry #42
tokf history show --raw 42     # print only the raw captured output
tokf history search "error"    # search by command or output content
tokf history clear             # clear current project history
tokf history clear --all       # clear all history (destructive)
```

## History hint

When an LLM receives filtered output it may not realise the full output exists. Two mechanisms can automatically append a hint line pointing to the history entry:

**1. Filter opt-in** — set `show_history_hint = true` in a filter TOML to always append the hint for that command:

```toml
command = "git status"
show_history_hint = true

[on_success]
output = "{branch} — {counts}"
```

**2. Automatic repetition detection** — tokf detects when the same command is run twice in a row for the same project. This is a signal the caller didn't act on the previous filtered output and may need the full content:

```
✓ cargo test: 42 passed (2.31s)
Filtered - full output: `tokf history show --raw 99`
```

The hint is appended to stdout so it is visible to both humans and LLMs in the tool output. The history entry itself always stores the clean filtered output, without the hint line.
