---
title: Generic Commands
description: Compress any command's output without a dedicated filter using tokf err, tokf test, and tokf summary.
order: 2
---

## Generic Commands

When no dedicated filter exists for a command, three built-in subcommands provide useful compression for arbitrary output:

| Command | Purpose | Default context |
|---------|---------|----------------|
| `tokf err <cmd>` | Extract errors and warnings | 3 lines |
| `tokf test <cmd>` | Extract test failures | 5 lines |
| `tokf summary <cmd>` | Heuristic summary | 30 lines max |

### `tokf err` — Error extraction

Scans output for error/warning patterns across common toolchains (Rust, Python, Node, Go, Java) and shows only the relevant lines with surrounding context.

```sh
# Show only errors from a build
tokf err cargo build

# Adjust context lines around each error
tokf err -C 5 cargo build

# Works with any command
tokf err python train.py
tokf err npm run build
```

**Patterns matched:** `error:`, `warning:`, `FAILED`, `Traceback`, `panic`, `npm ERR!`, `fatal:`, Python/Java exception types, and more.

**Behaviour:**
- Output < 10 lines: passed through unchanged
- No errors + exit 0: prints `[tokf err] no errors detected`
- No errors + exit ≠ 0: includes full output (something failed but no recognized pattern)

### `tokf test` — Test failure extraction

Extracts test failure details and always includes summary/result lines.

```sh
# Show only test failures
tokf test cargo test
tokf test go test ./...
tokf test npm test
tokf test pytest
```

**Patterns matched:** `FAIL`, `FAILED`, `panicked`, assertion mismatches, Jest `✕` markers, Go `--- FAIL:`, and more.

**Summary lines always included:** `test result:`, `Tests:`, `passed`, `failed` counts, pytest/RSpec summary lines.

**Behaviour:**
- Output < 10 lines: passed through unchanged
- All pass + exit 0: prints `[tokf test] all tests passed`
- Failures detected: shows failure lines with context + summary

### `tokf summary` — Heuristic summary

Produces a budget-constrained summary by identifying header, footer/summary, and repetitive middle sections.

```sh
# Summarize a long build log
tokf summary cargo build

# Limit to 15 lines
tokf summary --max-lines 15 make all
```

**Algorithm:**
1. Header (first 5 lines) and footer/summary (last lines matching keywords like "total", "finished", "result")
2. Middle section is sampled; highly repetitive content shows a count + samples
3. Extracted statistics (pass/fail counts, timing) appended as `[tokf summary]` line

### Common flags

All three commands support:

| Flag | Description |
|------|-------------|
| `--baseline-pipe <cmd>` | Fair baseline accounting (as with `tokf run`) |
| `--no-mask-exit-code` | Propagate the real exit code instead of masking to 0 |
| `--timing` | Show how long filtering took |

### Tracking and history

Generic commands record to the same tracking database and history as `tokf run`, using filter names `_builtin/err`, `_builtin/test`, and `_builtin/summary`. Use `tokf raw last` to see the full uncompressed output.
