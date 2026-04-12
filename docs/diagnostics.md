---
title: Diagnostics
description: Inspect your tokf setup, manage the filter cache, and troubleshoot.
order: 9
---

## tokf info

`tokf info` prints a summary of all paths, database locations, and filter counts. Useful for debugging when filters aren't being found or to verify your setup:

```sh
tokf info          # human-readable output
tokf info --json   # machine-readable JSON
```

Example output:

```
tokf 0.2.8
TOKF_HOME: (not set)

filter search directories:
  [local] /home/user/project/.tokf/filters (not found)
  [user] /home/user/.config/tokf/filters (not found)
  [built-in] <embedded> (always available)

tracking database:
  TOKF_DB_PATH: (not set)
  path: /home/user/.local/share/tokf/tracking.db (will be created)

filter cache:
  path: /home/user/.cache/tokf/manifest.bin (will be created)

filters:
  local:    0
  user:     0
  built-in: 38
  total:    38
```

### Environment variables

| Variable | Description | Default |
|---|---|---|
| `TOKF_HOME` | Redirect **all** user-level tokf paths (filters, cache, DB, hooks, auth) to a single directory | Platform config dir (e.g. `~/.config/tokf` on Linux) |
| `TOKF_DB_PATH` | Override the tracking database path only (takes precedence over `TOKF_HOME`) | Platform data dir (e.g. `~/.local/share/tokf/tracking.db`); or `$TOKF_HOME/tracking.db` when `TOKF_HOME` is set |
| `TOKF_NO_FILTER` | Skip filtering in shell mode (set to `1`, `true`, or `yes`) | unset |
| `TOKF_VERBOSE` | Print filter resolution details in shell mode | unset |

`TOKF_HOME` works like `CARGO_HOME` or `RUSTUP_HOME` — set it once to relocate everything:

```sh
# Put all tokf data under /opt/tokf (useful on read-only home dirs or shared systems)
TOKF_HOME=/opt/tokf tokf info

# Override only the tracking database, leave everything else in the default location
TOKF_DB_PATH=/tmp/my-tracking.db tokf info
```

The `tokf info` output always shows the active `TOKF_HOME` value (or `(not set)`) at the top,
so you can quickly verify which paths are in effect.

## Rewrite debugging

Use `tokf rewrite --verbose` to see how a command would be rewritten, including which rule fired:

```sh
tokf rewrite --verbose "make check"         # shows wrapper rule
tokf rewrite --verbose "cargo test"          # shows filter rule
tokf rewrite --verbose "cargo test | tail"   # shows pipe stripping
```

For shell mode (task runner recipe lines), set `TOKF_VERBOSE=1` to see filter resolution for each recipe line:

```sh
TOKF_VERBOSE=1 make check    # verbose output on stderr for each recipe
```

## tokf doctor

`tokf doctor` analyses your local `tracking.db` and reports filters that may be causing **agent confusion** — repeated calls, escape-flag usage, empty-output retries, or filters that are making output *bigger* than the raw command. It's the post-hoc complement to `tokf gain`: where `gain` measures how much you saved, `doctor` looks for places the savings were illusory because the agent had to retry.

```sh
tokf doctor                              # default: current project, table output
tokf doctor --json                       # machine-readable
tokf doctor --filter git/diff            # focus on one filter
tokf doctor --all                        # include events from every project
tokf doctor --burst-threshold 3 --window 30   # tighten the burst detector
tokf doctor --sort bursts                # sort by burst count instead of health
```

Example output:

```
tokf doctor — 41057 events, project=/Users/me/repo, threshold≥5 within 60s

filter                  events  score    bursts max-burst   retries
──────────────────────────────────────────────────────────────────────
git diff                  4056     20        67        17       304
git log                    920     35         9         8        46
git show                   183     45         2         3         0
git status                2418     85         1         5        12
cargo test                 412    100         0         -         -

retry-burst detail (top 5 by size)
  ×17 git diff <args> (git diff)
  ×12 git diff <args> (git diff)
  ×11 git diff <args> (git diff)
  ×10 git diff <args> (git diff)
  ×9 git diff <args> (git diff)

workaround-flag suggestions (consider adding to passthrough_args)
  git diff: --no-pager×35, --oneline×4
  git log: --no-pager×64, --pretty×4

filters with negative token savings (filtered output > raw)
  +122.8 avg tokens per call — git show
  +5.4 avg tokens per call — git log
```

### What each section means

| Section | What it detects | Threshold |
|---|---|---|
| **filter table** | Per-filter health summary, sorted by composite score (lower = worse) | `score = 100 − burst_penalty − workaround_penalty − empty_retry_penalty − negative_savings_penalty`, each capped so no single signal can crash the score on its own |
| **retry-burst detail** | The same exact command run ≥`--burst-threshold` (default `5`) times within `--window` seconds (default `60`). Shows top 5 burst sessions by size. | Strong signal that the model didn't believe / couldn't read the filtered output and kept trying |
| **workaround-flag suggestions** | Flags like `--no-stat`, `--no-pager`, `-p`, `--name-only`, `--format` that appear often **but are not declared in the filter's `passthrough_args`** | Each occurrence is the agent trying to escape the filter; if a flag appears repeatedly, the filter probably should add it to `passthrough_args` |
| **filters with negative token savings** | Filters where the average filtered output is **larger** than the raw command output | Usually caused by `on_empty` adding explanatory text to a small command, or stat tables expanding short diffs. The fix is filter-specific |

### Multi-project handling

`tracking.db` records the project root for every event (resolved by walking up from the cwd looking for `.git` / `.tokf`). By default, `tokf doctor` scopes its analysis to the current project. Use:

- `--project /path/to/repo` — analyse a specific project
- `--all` — analyse events from every project together

Events recorded **before** the project column was added (legacy rows in upgraded DBs) are visible from every scope until they age out naturally.

### Noise filtering

The doctor excludes events whose command path looks like a temp-dir or test-fixture invocation by default — `/var/folders/...`, `/tmp/...`, `.tokf-verify-...`, etc. These are usually statusline / shell-prompt callers, `tokf verify` rigs, or hook scripts running before/after every tool call, none of which are agent confusion. Use `--include-noise` to disable the filter when you want to see *everything*.

### What's not included (yet)

`tokf doctor` is the **post-hoc** half of the diagnostics story. Phase 2 will add **runtime surfacing** — an in-process LRU that detects bursts as they happen and prints a `[tokf] notice:` line on stderr in the same tool result the agent sees. Phase 3 will add an `--apply-suggestions` interactive mode that proposes config patches. Both are explicitly out of scope for the current release.

## Cache management

tokf caches the filter discovery index for faster startup. The cache rebuilds automatically when filters change, but you can manage it manually:

```sh
tokf cache info    # show cache location, size, and validity
tokf cache clear   # delete the cache, forcing a rebuild on next run
```

## Shell completions

Generate tab-completion scripts for your shell:

```sh
tokf completions bash
tokf completions zsh
tokf completions fish
tokf completions powershell
tokf completions elvish
tokf completions nushell
```

### Installation

**Bash** — add to `~/.bashrc`:
```sh
eval "$(tokf completions bash)"
```

**Zsh** — add to `~/.zshrc`:
```sh
eval "$(tokf completions zsh)"
```

**Fish** — save to completions directory:
```sh
tokf completions fish > ~/.config/fish/completions/tokf.fish
```

**PowerShell** — add to your profile:
```powershell
tokf completions powershell | Out-String | Invoke-Expression
```

**Elvish** — add to `~/.elvish/rc.elv`:
```sh
eval (tokf completions elvish | slurp)
```

**Nushell** — save and source in your config:
```sh
tokf completions nushell | save -f ~/.config/nushell/tokf.nu
source ~/.config/nushell/tokf.nu
```
