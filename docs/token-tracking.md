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

## How tokens are estimated

tokf does not run a tokenizer. Token counts are derived from byte counts with one constant:

```
tokens ≈ bytes / 3.5
```

That is why every token figure `tokf gain` prints is labelled **`est.`**, and why it will keep being labelled that way for as long as this estimator ships. It is a heuristic, not a measurement.

### Where 3.5 comes from

The constant used to be `4`, and nothing had ever checked it. It was measured against a real cl100k tokenizer across the whole tokf corpus — every filter `_test/` case (both the raw input and the filtered output) plus every fixture under `tests/fixtures/` — which gives the *implied* divisor, i.e. bytes per real token:

| corpus | implied divisor |
|---|---|
| raw command output | 3.67 |
| filtered output | 2.98 |
| combined (byte-weighted) | 3.53 |
| spread across items | p10 2.72 · median 3.39 · p90 4.62 |

`3.5` is the combined figure rounded. The old `4` undercounted real cl100k tokens by roughly 8% on raw output and 25% on filtered output; `3.5` lands within 1% of the corpus aggregate.

Two honest caveats:

- **cl100k is not Claude's tokenizer.** Even the "real" numbers we calibrated against are an approximation of the thing users actually care about. The goal was removing a large systematic bias, not achieving exactness.
- **The corpus is what it is** — heavily weighted toward `cargo`, `git`, `npm` and `docker` output. The p10/p90 spread above shows the per-item divisor genuinely ranges from under 2.8 to over 4.6 depending on output shape: prose is cheap per byte to tokenize, dense symbolic output is expensive. One constant cannot capture that, and tokf deliberately does not try. It says `est.` instead.

### Percentages are more reliable than absolute counts — except when filters rewrite

The savings *percentage* divides two counts that share the divisor, so most of the error cancels out. That holds well for filters that mostly **delete** lines:

| case | est. reduction | real reduction | error |
|---|---|---|---|
| `cargo/check` (successful check collapses) | 84.4% | 84.6% | −0.2 pt |
| `cargo/clippy` (grouped by lint rule) | 78.5% | 78.2% | +0.3 pt |
| `cargo/build` (failure output) | 23.3% | 24.2% | −0.9 pt |

It stops holding for filters that **rewrite** content — replacing English prose with a dense symbolic summary. Prose tokenizes cheaply per byte; the summary does not, so the byte ratio overstates the token ratio and the filter flatters itself:

| case | est. reduction | real reduction | error |
|---|---|---|---|
| `docker/ps` (no running containers → zero count) | 0.0% | −300.0% | +300 pt |
| `git/push` (up-to-date → friendly message) | 33.3% | −50.0% | +83 pt |
| `git/status` (clean repo → branch marker) | 50.0% | 0.0% | +50 pt |

Those are worst cases on tiny outputs, where a handful of tokens swings the percentage wildly — but the direction of the bias is consistent, and it is upward. **Treat reduction percentages on rewriting filters as indicative, not as a claim.**

### Verifying the estimate yourself

The tokenizer is a contributor tool, not a runtime feature. It sits behind an optional cargo feature that is **off by default**, so normal builds take no tokenizer dependency at all:

```sh
cargo test -p tokf --features tokenizer --test calibration -- --ignored --nocapture
```

That prints the full per-item table and the aggregates above, and fails if the shipped constant drifts more than 25% from what the corpus implies. There is no way to enable a real tokenizer at runtime, and no plan to add one — carrying a vocabulary table in the shipping binary to serve a statistic is not a trade tokf wants to make.

### Estimates changed: a deliberate discontinuity

Changing the divisor changes the numbers. Being explicit about what that means:

- Rows recorded **before** the change keep their old `bytes / 4` token counts. They are not rewritten.
- Rows recorded **after** use `bytes / 3.5`.
- `tokf gain` totals spanning the changeover therefore show a **step increase in absolute token counts**. Nothing is being saved differently; the estimate simply got less wrong.
- Savings **percentages** are essentially unaffected, because both sides of the ratio scale together. That is the reason the discontinuity is tolerable.
- The same applies to server-side aggregates, which receive already-computed token columns via sync.

We deliberately did **not**: version the estimator in the SQLite schema (real surface area across three crates for a statistic), rewrite historical rows (local history would then diverge from already-synced server rows — worse than one honest step), or recompute tokens at read time (touches every aggregate query and still cannot fix the server side). One documented step change beat all three.

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

The `🗜️` prefix appears on all filtered output (disable with `tokf config set output.show_indicator false` or `TOKF_SHOW_INDICATOR=false`). The hint line is appended to stdout so it is visible to both humans and LLMs in the tool output. The history entry itself always stores the clean filtered output, without the hint line or indicator.

## Context injection

During `tokf hook install`, tokf creates a `.claude/TOKF.md` file and adds an `@TOKF.md` reference to `.claude/CLAUDE.md`. This gives LLMs a two-line context explaining what `🗜️` means and how to retrieve full output (`tokf raw last`). Use `--no-context` to skip this step.
