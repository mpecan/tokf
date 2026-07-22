---
title: Token Savings Tracking
description: See exactly how much context tokf saves you, run by run.
order: 5
---

tokf records input/output byte counts per run in a local SQLite database:

```sh
tokf gain              # summary: total bytes saved and reduction %
tokf gain --daily      # day-by-day breakdown
tokf gain --by-filter  # breakdown by filter
tokf gain --json       # machine-readable output
```

## Remote gain

View aggregate savings across all your registered machines via the tokf server:

```sh
tokf gain --remote              # summary across all machines
tokf gain --remote --by-filter  # breakdown by filter
tokf gain --remote --json       # machine-readable output
```

Remote gain requires authentication (`tokf auth login`). The `--daily` flag is not available remotely. See [Remote Sharing](#remote-sharing) for the full setup workflow.

## Output history

tokf records raw and filtered outputs in a local SQLite database, useful for debugging filters or reviewing what an AI agent saw:

```sh
tokf raw last                  # print raw output of last filtered command
tokf raw 42                    # print raw output of entry #42
tokf history list              # recent entries (current project)
tokf history list -l 20        # show 20 entries
tokf history list --all        # entries from all projects
tokf history show 42           # full details for entry #42
tokf history show --raw 42     # print only the raw captured output (long form)
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
🗜️ ✓ cargo test: 42 passed (2.31s)
🗜️ compressed — run `tokf raw 99` for full output
```

The `🗜️` prefix appears on all filtered output (disable with `tokf config set output.show_indicator false` or `TOKF_SHOW_INDICATOR=false`). The hint line is appended to stdout so it is visible to both humans and LLMs in the tool output. The history entry itself always stores the clean filtered output, without the hint line, indicator or recovery marker.

## Per-entry recovery markers

When a filtered command is recorded in history, the indicator carries that entry's ID directly:

```
🗜️#87 ✓ cargo test: 42 passed
```

`🗜️#87` means the full, unfiltered output is one command away — `tokf raw 87`. Without an ID (`🗜️` alone) the run was not recorded, so there is nothing to recover.

This is **additive**: the filtered body is byte-identical to what tokf printed before, and the ID rides the indicator that was already being printed. It costs roughly three tokens; there is no extra line and no extra newline. Disabling the indicator (`output.show_indicator = false`) removes the marker too — the ID is not smuggled back in on its own line.

### Why the CLI rather than a tool call

Recovery is deliberately a shell command. `tokf raw <id>` composes:

```sh
tokf raw 87 | grep -n 'error\[' | head -20
```

A recovered entry can be enormous — a `cargo metadata` capture runs to hundreds of thousands of tokens — so being able to narrow it *before* it reaches the model matters. Output that arrives through a tool call lands in context whole, with no opportunity to filter it first. Piping also means the recovered text can itself be filtered by tokf.

Prefer `tokf raw <id> | ...` over reading an entry whole.

### Why the ID is decimal

Decimal is the cheapest encoding, which is counterintuitive — a shorter string is not a smaller number of tokens. BPE tokenizers pack runs of digits (up to three per token) while mixed-case alphanumerics fragment. Measured against `cl100k`:

| id | decimal | base36 | base62 |
|---|---|---|---|
| 142 | **1** | 2 | 2 |
| 4821 | **2** | 2 | 2 |
| 51234 | **2** | 3 | **2** |
| 998877 | **2** | 2 | 3 |

Decimal is never worse and sometimes better, so the ID is printed as-is.

## Context injection

During `tokf hook install`, tokf creates a `.claude/TOKF.md` file and adds an `@TOKF.md` reference to `.claude/CLAUDE.md`. This gives LLMs a short context explaining what `🗜️` and `🗜️#<id>` mean and how to retrieve full output (`tokf raw <id>`, or `tokf raw last`). Use `--no-context` to skip this step.
