---
title: Rewrite Configuration
description: Control how tokf rewrites commands with rewrites.toml, pipe stripping, and environment variable handling.
order: 6
---

## Rewrite configuration (`rewrites.toml`)

tokf looks for a `rewrites.toml` file in two locations (first found wins):

1. **Project-local**: `.tokf/rewrites.toml` — scoped to the current repository
2. **User-level**: `~/.config/tokf/rewrites.toml` — applies to all projects

This file controls custom rewrite rules, skip patterns, and pipe handling. All `[pipe]`, `[skip]`, and `[[rewrite]]` sections documented below go in this file.

## Task runner integration (make, just)

Task runners like `make` and `just` execute recipe lines via a shell (`$SHELL -c 'recipe_line'`). By default, only the outer `make`/`just` command is visible to tokf — child commands (`cargo test`, `uv run mypy`, etc.) pass through unfiltered.

tokf solves this with **built-in wrapper rules** that inject tokf as the task runner's shell. Each recipe line is then individually matched against installed filters:

```sh
# What you type:
make check

# What tokf rewrites it to:
make SHELL=tokf check

# What make then does for each recipe line:
tokf -c 'cargo test'          → filter matches → filtered output
tokf -c 'cargo clippy'        → filter matches → filtered output
tokf -c 'echo done'           → no filter → delegates to sh
```

For `just`, the `--shell` flag is used instead:

```sh
just test  →  just --shell tokf --shell-arg -cu test
```

### Exit code preservation

Shell mode (`tokf -c '...'`) always propagates the **real exit code** — no masking, no "Error: Exit code N" prefix. This means `make` sees the actual exit code from each recipe line and stops on failure as expected.

### Shell mode (`tokf -c`)

When invoked as `tokf -c 'command'` (or with combined flags like `-cu`, `-ec`), tokf enters shell mode. It tries to match the command against installed filters. If a match is found, the command runs through tokf's filter pipeline and filtered output is printed. If no match is found, the command is delegated to `sh` with the same flags — so unfiltered recipes run normally.

This mode is not typically invoked directly; it is called by task runners (make, just) after the rewrite injects tokf as their shell.

### Compound and complex recipe lines

Recipe lines with shell metacharacters — operators (`&&`, `||`, `;`), pipes (`|`), redirections (`>`, `<`), quotes, globs, or subshells — are delegated to the real shell (`sh`) so that their semantics are preserved. (Operators inside quoted strings may also trigger delegation — this is a safe false positive since `sh` handles them correctly.) Only simple `command arg arg` recipe lines are matched against filters.

### Debugging task runner rewrites

Use `tokf rewrite --verbose "make check"` to confirm the wrapper rewrite is active and see which rule fired.

Shell mode also respects environment variables for diagnostics (since it has no access to CLI flags like `--verbose`):

```sh
TOKF_VERBOSE=1 make check     # print filter resolution details for each recipe line
TOKF_NO_FILTER=1 make check   # bypass filtering entirely, delegate all recipe lines to sh
```

### Overriding or disabling wrappers

The built-in wrappers for `make` and `just` can be overridden or disabled via `[[rewrite]]` or `[skip]` entries in `.tokf/rewrites.toml`:

```toml
# Override the make wrapper with a custom one:
# "make check" → "make SHELL=tokf .SHELLFLAGS=-ec check"
# Note: use (?:[^\\s]*/)? prefix to also match full paths like /usr/bin/make
[[rewrite]]
match = "^(?:[^\\s]*/)?make(\\s.*)?$"
replace = "make SHELL=tokf .SHELLFLAGS=-ec{1}"

# Or disable it entirely:
[skip]
patterns = ["^make"]
```

### Adding wrappers for other task runners

You can add wrappers for other task runners via `[[rewrite]]`. The exact mechanism depends on how the task runner invokes recipe lines — check its documentation for shell override options:

```toml
# Example: if your task runner respects $SHELL for recipe execution
[[rewrite]]
match = "^(?:[^\\s]*/)?mise run(\\s.*)?$"
replace = "SHELL=tokf mise run{1}"
```

## Piped commands

When a command is piped to a simple output-shaping tool (`grep`, `tail`, or `head`), tokf **strips the pipe automatically** and uses its own structured filter output instead. The original pipe suffix is passed to `--baseline-pipe` so token savings are still calculated accurately.

```sh
# These ARE rewritten — pipe is stripped, tokf applies its filter:
cargo test | grep FAILED
cargo test | tail -20
git diff HEAD | head -5
```

Multi-pipe chains, pipes to other commands, or pipe targets with unsupported flags are left unchanged:

```sh
# These are NOT rewritten — tokf leaves them alone:
kubectl get pods | grep Running | wc -l   # multi-pipe chain
cargo test | wc -l                        # wc not supported
cargo test | tail -f                      # -f (follow) not supported
```

If you want tokf to wrap a piped command that wouldn't normally be rewritten, add an explicit rule to `.tokf/rewrites.toml`:

```toml
[[rewrite]]
match = "^cargo test \\| tee"
replace = "tokf run {0}"
```

Use `tokf rewrite --verbose "cargo test | grep FAILED"` to see how a command is being rewritten.

### Disabling pipe stripping

If you prefer tokf to never strip pipes (leaving piped commands unchanged), add a `[pipe]` section to `.tokf/rewrites.toml`:

```toml
[pipe]
strip = false   # default: true
```

When `strip = false`, commands like `cargo test | tail -5` pass through the shell unchanged. Non-piped commands are still rewritten normally.

### Prefer less context mode

Sometimes the piped output (e.g. `tail -5`) is actually smaller than the filtered output. The `prefer_less` option tells tokf to compare both at runtime and use whichever is smaller:

```toml
[pipe]
prefer_less = true   # default: false
```

When a pipe is stripped, tokf injects `--prefer-less` alongside `--baseline-pipe`. At runtime:
1. The filter runs normally
2. The original pipe command also runs on the raw output
3. tokf prints whichever result is smaller

When the pipe output wins, the event is recorded with `pipe_override = 1` in the tracking DB. The `tokf gain` command shows how many times this happened:

```
tokf gain summary
  total runs:     42
  input tokens:   12,500 est.
  output tokens:  3,200 est.
  tokens saved:   9,300 est. (74.4%)
  pipe preferred: 5 runs (pipe output was smaller than filter)
```

Note: `strip = false` takes priority — if pipe stripping is disabled, `prefer_less` has no effect.

## Environment variable prefixes

Leading `KEY=VALUE` assignments are automatically stripped before matching, so env-prefixed commands are rewritten correctly:

```sh
# These ARE rewritten — env vars are preserved, the command is wrapped:
DEBUG=1 git status              → DEBUG=1 tokf run git status
RUST_LOG=debug cargo test       → RUST_LOG=debug tokf run cargo test
A=1 B=2 cargo test | tail -5   → A=1 B=2 tokf run --baseline-pipe 'tail -5' cargo test
```

The env vars are passed through verbatim to the underlying command; tokf only rewrites the executable portion.

### Skip patterns and env var prefixes

User-defined skip patterns in `.tokf/rewrites.toml` match against the **full** shell segment, including any leading env vars. A pattern `^cargo` will **not** skip `RUST_LOG=debug cargo test` because the segment doesn't start with `cargo`:

```toml
[skip]
patterns = ["^cargo"]   # skips "cargo test" but NOT "RUST_LOG=debug cargo test"
```

To skip a command regardless of any env prefix, use a pattern that accounts for it:

```toml
[skip]
patterns = ["(?:^|\\s)cargo\\s"]   # matches "cargo" anywhere after start or whitespace
```
