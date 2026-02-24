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
