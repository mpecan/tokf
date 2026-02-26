# tokf

[![CI](https://github.com/mpecan/tokf/actions/workflows/ci.yml/badge.svg)](https://github.com/mpecan/tokf/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/tokf)](https://crates.io/crates/tokf)
[![crates.io downloads](https://img.shields.io/crates/d/tokf)](https://crates.io/crates/tokf)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

**[tokf.net](https://tokf.net)** — reduce LLM context consumption from CLI commands by 60–90%.

Commands like `git push`, `cargo test`, and `docker build` produce verbose output packed with progress bars, compile noise, and boilerplate. tokf intercepts that output, applies a TOML filter, and emits only what matters — so your AI agent sees a clean signal instead of hundreds of wasted tokens.

---

## Before / After

**`cargo test` — 61 lines → 1 line:**

<table>
<tr>
<th>Without tokf</th>
<th>With tokf</th>
</tr>
<tr>
<td>

```
   Compiling tokf v0.2.0 (/home/user/tokf)
   Compiling proc-macro2 v1.0.92
   Compiling unicode-ident v1.0.14
   Compiling quote v1.0.38
   Compiling syn v2.0.96
   Compiling serde_derive v1.0.217
   Compiling serde v1.0.217
   ...
running 47 tests
test config::tests::test_load ... ok
test filter::tests::test_skip ... ok
test filter::tests::test_keep ... ok
test filter::tests::test_extract ... ok
...
test result: ok. 47 passed; 0 failed; 0 ignored
  finished in 2.31s
```

</td>
<td>

```
✓ 47 passed (2.31s)
```

</td>
</tr>
</table>

**`git push` — 8 lines → 1 line:**

<table>
<tr>
<th>Without tokf</th>
<th>With tokf</th>
</tr>
<tr>
<td>

```
Enumerating objects: 5, done.
Counting objects: 100% (5/5), done.
Delta compression using up to 10 threads
Compressing objects: 100% (3/3), done.
Writing objects: 100% (3/3), 312 bytes | 312.00 KiB/s, done.
Total 3 (delta 2), reused 0 (delta 0), pack-reused 0
remote: Resolving deltas: 100% (2/2), completed with 2 local objects.
To github.com:user/repo.git
   a1b2c3d..e4f5a6b  main -> main
```

</td>
<td>

```
ok ✓ main
```

</td>
</tr>
</table>

---


## Installation

### Homebrew (macOS and Linux)

```sh
brew install mpecan/tokf/tokf
```

### cargo

```sh
cargo install tokf
```

### Build from source

```sh
git clone https://github.com/mpecan/tokf
cd tokf
cargo build --release
# binary at target/release/tokf
```

---

## How it works

```
tokf run git push origin main
```

tokf looks up a filter for `git push`, runs the command, and applies the filter. The filter logic lives in plain TOML files — no recompilation required. Anyone can author, share, or override a filter.

---

## Usage

### Run a command with filtering

```sh
tokf run git push origin main
tokf run cargo test
tokf run docker build .
```

### Test a filter against a fixture

```sh
tokf test filters/git/push.toml tests/fixtures/git_push_success.txt --exit-code 0
```

### Verify filter test suites

```sh
tokf verify                    # run all test suites
tokf verify git/push           # run a specific suite
tokf verify --list             # list available suites and case counts
tokf verify --json             # output results as JSON
tokf verify --require-all      # fail if any filter has no test suite
tokf verify --list --require-all  # show coverage per filter
tokf verify --scope project    # only project-local filters (.tokf/filters/)
tokf verify --scope global     # only user-level filters (~/.config/tokf/filters/)
tokf verify --scope stdlib     # only built-in stdlib (filters/ in CWD)
```

### Explore available filters

```sh
tokf ls                    # list all filters
tokf which "cargo test"    # which filter would match
tokf show git/push         # print the TOML source
```

### Customize a built-in filter

```sh
tokf eject cargo/build            # copy to .tokf/filters/ (project-local)
tokf eject cargo/build --global   # copy to ~/.config/tokf/filters/ (user-level)
```

This copies the filter TOML and its test suite to your config directory, where it shadows the built-in. Edit the ejected copy freely — tokf's priority system ensures your version is used instead of the original.

### Flags

| Flag | Description |
|---|---|
| `--timing` | Print how long filtering took |
| `--verbose` | Show which filter was matched (also explains skipped rewrites) |
| `--no-filter` | Pass output through without filtering |
| `--no-cache` | Bypass the filter discovery cache |
| `--no-mask-exit-code` | Disable exit-code masking. By default tokf exits 0 and prepends `Error: Exit code N` on failure |
| `--preserve-color` | Preserve ANSI color codes in filtered output (env: `TOKF_PRESERVE_COLOR=1`). See [Color passthrough](#color-passthrough) below |
| `--baseline-pipe` | Pipe command for fair baseline accounting (injected by rewrite) |
| `--prefer-less` | Compare filtered vs piped output and use whichever is smaller (requires `--baseline-pipe`) |

### Color passthrough

By default, filters with `strip_ansi = true` permanently remove ANSI escape codes. The `--preserve-color` flag changes this: tokf strips ANSI **internally** for pattern matching (skip, keep, dedup) but restores the original colored lines in the final output. When `--preserve-color` is active it overrides `strip_ansi = true` in the filter config.

tokf does **not** force commands to emit color — you must ensure the child command outputs ANSI codes (e.g. via `FORCE_COLOR=1` or `--color=always`):

```sh
# Node.js / Vitest / Jest
FORCE_COLOR=1 tokf run --preserve-color npm test

# Cargo
tokf run --preserve-color cargo test -- --color=always

# Or set the env var once for all invocations
export TOKF_PRESERVE_COLOR=1
FORCE_COLOR=1 tokf run npm test
```

**Limitations:** color passthrough applies to the skip/keep/dedup pipeline (stages 2–2.5). The `match_output`, `parse`, and `lua_script` stages operate on clean text and are unaffected by this flag. `[[replace]]` rules run on the raw text before the color split, so when `--preserve-color` is enabled their patterns may need to account for ANSI escape codes, similar to branch-level `skip` patterns, which also match against the restored colored text.

---

## Built-in filter library

| Filter | Command |
|---|---|
| `git/add` | `git add` |
| `git/commit` | `git commit` |
| `git/diff` | `git diff` |
| `git/log` | `git log` |
| `git/push` | `git push` |
| `git/show` | `git show` |
| `git/status` | `git status` — runs `git status --porcelain -b`; shows branch name + one porcelain-format line per changed file (e.g. `M  src/main.rs`, `?? scratch.rs`) |
| `cargo/build` | `cargo build` |
| `cargo/check` | `cargo check` |
| `cargo/clippy` | `cargo clippy` |
| `cargo/fmt` | `cargo fmt` |
| `cargo/install` | `cargo install *` |
| `cargo/test` | `cargo test` |
| `docker/*` | `docker build`, `docker compose`, `docker images`, `docker ps` |
| `npm/run` | `npm run *` |
| `npm/test` | `npm test`, `pnpm test`, `yarn test` (with vitest/jest variants) |
| `pnpm/*` | `pnpm add`, `pnpm install` |
| `go/*` | `go build`, `go vet` |
| `gradle/*` | `gradle build`, `gradle test`, `gradle dependencies` |
| `gh/*` | `gh pr list`, `gh pr view`, `gh pr checks`, `gh issue list`, `gh issue view` |
| `kubectl/*` | `kubectl get pods` |
| `next/*` | `next build` |
| `prisma/*` | `prisma generate` |
| `pytest` | Python test runner |
| `tsc` | TypeScript compiler |
| `ls` | `ls` |

---


Filters are TOML files placed in `.tokf/filters/` (project-local) or `~/.config/tokf/filters/` (user-level). Project-local filters take priority over user-level, which take priority over the built-in library.

## Minimal example

```toml
command = "my-tool"

[on_success]
output = "ok ✓"

[on_failure]
tail = 10
```

## Command matching

tokf matches commands against filter patterns using two built-in behaviours:

**Basename matching** — the first word of a pattern is compared by basename, so a filter with `command = "git push"` will also match `/usr/bin/git push` or `./git push`.  This works automatically; no special pattern syntax is required.

**Transparent global flags** — flag-like tokens between the command name and a subcommand keyword are skipped during matching.  A filter for `git log` will match all of:

```
git log
git -C /path log
git --no-pager -C /path log --oneline
/usr/bin/git --no-pager -C /path log
```

The skipped flags are preserved in the command that actually runs — they are only bypassed during the pattern match.

> **Note on `run` override and transparent flags:** If a filter sets a `run` field, transparent global flags are *not* included in `{args}`.  Only the arguments that appear after the matched pattern words are available as `{args}`.

## Common fields

```toml
command = "git push"          # command pattern to match (supports wildcards and arrays)
run = "git push {args}"       # override command to actually execute

skip = ["^Enumerating", "^Counting"]  # drop lines matching these regexes
keep = ["^error"]                      # keep only lines matching (inverse of skip)

# Per-line regex replacement — applied before skip/keep, in order.
# Capture groups use {1}, {2}, … . Invalid patterns are silently skipped.
[[replace]]
pattern = '^(\S+)\s+\S+\s+(\S+)\s+(\S+)'
output = "{1}: {2} → {3}"

dedup = true                  # collapse consecutive identical lines
dedup_window = 10             # optional: compare within a N-line sliding window

strip_ansi = true             # strip ANSI escape sequences before processing
trim_lines = true             # trim leading/trailing whitespace from each line
strip_empty_lines = true      # remove all blank lines from the final output
collapse_empty_lines = true   # collapse consecutive blank lines into one

show_history_hint = true      # append a hint line pointing to the full output in history

match_output = [              # whole-output substring checks, short-circuit the pipeline
  { contains = "rejected", output = "push rejected" },
]

[on_success]                  # branch for exit code 0
output = "ok ✓ {2}"          # template; {output} = pre-filtered output

[on_failure]                  # branch for non-zero exit
tail = 10                     # keep the last N lines
```

## Template pipes

Output templates support pipe chains: `{var | pipe | pipe: "arg"}`.

| Pipe | Input → Output | Description |
|---|---|---|
| `join: "sep"` | Collection → Str | Join items with separator |
| `each: "tmpl"` | Collection → Collection | Map each item through a sub-template |
| `truncate: N` | Str → Str | Truncate to N characters, appending `…` |
| `lines` | Str → Collection | Split on newlines |
| `keep: "re"` | Collection → Collection | Retain items matching the regex |
| `where: "re"` | Collection → Collection | Alias for `keep:` |

Example — filter a multi-line output variable to only error lines:

```toml
[on_failure]
output = "{output | lines | keep: \"^error\" | join: \"\\n\"}"
```

Example — for each collected block, show only `>` (pointer) and `E` (assertion) lines:

```toml
[on_failure]
output = "{failure_lines | each: \"{value | lines | keep: \\\"^[>E] \\\"}\" | join: \"\\n\"}"
```

## Filter variants

Some commands are wrappers around different underlying tools (e.g. `npm test` may run Jest, Vitest, or Mocha). A parent filter can declare `[[variant]]` entries that delegate to specialized child filters based on project context:

```toml
command = ["npm test", "pnpm test", "yarn test"]

strip_ansi = true
skip = ["^> ", "^\\s*npm (warn|notice|WARN|verbose|info|timing|error|ERR)"]

[on_success]
output = "{output}"

[on_failure]
tail = 20

[[variant]]
name = "vitest"
detect.files = ["vitest.config.ts", "vitest.config.js", "vitest.config.mts"]
filter = "npm/test-vitest"

[[variant]]
name = "jest"
detect.files = ["jest.config.js", "jest.config.ts", "jest.config.json"]
filter = "npm/test-jest"
```

Detection is two-phase:

1. **File detection** (before execution) — checks if config files exist in the current directory. First match wins.
2. **Output pattern** (after execution) — regex-matches command output. Used as a fallback when no file was detected.

When no variant matches, the parent filter's own fields (`skip`, `on_success`, etc.) apply as the fallback.

The `filter` field references another filter by its discovery name (relative path without `.toml`). Use `tokf which "npm test" -v` to see variant resolution.

> **TOML ordering**: `[[variant]]` entries must appear **after** all top-level fields (`skip`, `[on_success]`, etc.) because TOML array-of-tables sections capture subsequent keys.

## Filter resolution

1. `.tokf/filters/` in the current directory (repo-local overrides)
2. `~/.config/tokf/filters/` (user-level overrides)
3. Built-in library (embedded in the binary)

First match wins. Use `tokf which "git push"` to see which filter would activate.

## Writing test cases

Filter tests live in a `<stem>_test/` directory adjacent to the filter TOML:

```
filters/
  git/
    push.toml          <- filter config
    push_test/         <- test suite
      success.toml
      rejected.toml
```

Each test case is a TOML file specifying a fixture (inline or file path), expected exit code, and one or more `[[expect]]` assertions:

```toml
name = "rejected push shows pull hint"
fixture = "tests/fixtures/git_push_rejected.txt"
exit_code = 1

[[expect]]
equals = "✗ push rejected (try pulling first)"
```

For quick inline fixtures without a file:

```toml
name = "clean tree shows nothing to commit"
inline = "## main...origin/main\n"
exit_code = 0

[[expect]]
contains = "clean"
```

**Assertion types**:

| Field | Description |
|---|---|
| `equals` | Output exactly equals this string |
| `contains` | Output contains this substring |
| `not_contains` | Output does not contain this substring |
| `starts_with` | Output starts with this string |
| `ends_with` | Output ends with this string |
| `line_count` | Output has exactly N non-empty lines |
| `matches` | Output matches this regex |
| `not_matches` | Output does not match this regex |

Exit codes from `tokf verify`: `0` = all pass, `1` = assertion failure, `2` = config/IO error or uncovered filters (`--require-all`).

---


For logic that TOML can't express — numeric math, multi-line lookahead, conditional branching — embed a [Luau](https://luau.org/) script:

```toml
command = "my-tool"

[lua_script]
lang = "luau"
source = '''
if exit_code == 0 then
    return "passed"
else
    return "FAILED: " .. output:match("Error: (.+)") or output
end
'''
```

Available globals: `output` (string), `exit_code` (integer — the underlying command's real exit code, unaffected by `--no-mask-exit-code`), `args` (table).
Return a string to replace output, or `nil` to fall through to the rest of the TOML pipeline.
The sandbox blocks `io`, `os`, and `package` — no filesystem or network access from scripts.

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
[tokf] output filtered — to see what was omitted: `tokf history show --raw 99`
```

The hint is appended to stdout so it is visible to both humans and LLMs in the tool output. The history entry itself always stores the clean filtered output, without the hint line.

---


## Claude Code hook

tokf integrates with [Claude Code](https://claude.ai/code) as a `PreToolUse` hook that **automatically filters every `Bash` tool call** — no changes to your workflow required.

```sh
tokf hook install          # project-local (.tokf/)
tokf hook install --global # user-level (~/.config/tokf/)
```

Once installed, every command Claude runs through the Bash tool is filtered transparently. Track cumulative savings with `tokf gain`.

tokf also ships a filter-authoring skill that teaches Claude the complete filter schema:

```sh
tokf skill install          # project-local (.claude/skills/)
tokf skill install --global # user-level (~/.claude/skills/)
```

## OpenCode

tokf integrates with [OpenCode](https://opencode.ai) via a plugin that applies filters in real-time before command execution.

**Requirements:** OpenCode with Bun runtime installed.

**Install (project-local):**
```sh
tokf hook install --tool opencode
```

**Install (global):**
```sh
tokf hook install --tool opencode --global
```

This writes `.opencode/plugins/tokf.ts` (or `~/.config/opencode/plugins/tokf.ts` for `--global`), which OpenCode auto-loads. The plugin uses OpenCode's `tool.execute.before` hook to intercept `bash` tool calls and rewrites the command in-place when a matching filter exists. **Restart OpenCode after installation for the plugin to take effect.**

If tokf rewrite fails or no filter matches, the command passes through unmodified (fail-safe).

## OpenAI Codex CLI

tokf integrates with [OpenAI Codex CLI](https://github.com/openai/codex) via a skill that instructs the agent to prefix supported commands with `tokf run`.

**Install (project-local):**
```sh
tokf hook install --tool codex
```

**Install (global):**
```sh
tokf hook install --tool codex --global
```

This writes `.agents/skills/tokf-run/SKILL.md` (or `~/.agents/skills/tokf-run/SKILL.md` for `--global`), which Codex auto-discovers. Unlike the Claude Code hook (which intercepts commands at the tool level), the Codex integration is skill-based: it teaches the agent to use `tokf run` as a command prefix. If tokf is not installed, the agent falls back to running commands without the prefix (fail-safe).

## Creating Filters with Claude

tokf ships a Claude Code skill that teaches Claude the complete filter schema, processing order, step types, template pipes, and naming conventions.

**Invoke automatically**: Claude will activate the skill whenever you ask to create or modify a filter — just describe what you want in natural language:

> "Create a filter for `npm install` output that keeps only warnings and errors"
> "Write a tokf filter for `pytest` that shows a summary on success and failure details on fail"

**Invoke explicitly** with the `/tokf-filter` slash command:

```
/tokf-filter create a filter for docker build output
```

The skill is in `.claude/skills/tokf-filter/SKILL.md`. Reference material (exhaustive step docs and an annotated example TOML) lives in `.claude/skills/tokf-filter/references/`.

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

`TOKF_HOME` works like `CARGO_HOME` or `RUSTUP_HOME` — set it once to relocate everything:

```sh
# Put all tokf data under /opt/tokf (useful on read-only home dirs or shared systems)
TOKF_HOME=/opt/tokf tokf info

# Override only the tracking database, leave everything else in the default location
TOKF_DB_PATH=/tmp/my-tracking.db tokf info
```

The `tokf info` output always shows the active `TOKF_HOME` value (or `(not set)`) at the top,
so you can quickly verify which paths are in effect.

## Cache management

tokf caches the filter discovery index for faster startup. The cache rebuilds automatically when filters change, but you can manage it manually:

```sh
tokf cache info    # show cache location, size, and validity
tokf cache clear   # delete the cache, forcing a rebuild on next run
```

---


## Overview

The tokf community registry lets you discover filters published by other users, install them
locally, and share your own filters with the community.

Authentication is required for all registry operations. Run `tokf auth login` first.

---

## Searching for Filters

```sh
tokf search <query>
```

Returns filters whose command pattern matches `<query>` as a substring, ranked by token savings
and install count.

```
COMMAND              AUTHOR    SAVINGS%   INSTALLS
git push             alice       42.3%     1,234
git push --force     bob         38.1%       891
```

### Options

| Flag | Description |
|------|-------------|
| `-n, --limit <N>` | Maximum results to return (default: 20, max: 100) |
| `--json` | Output raw JSON array |

### Examples

```sh
tokf search git              # find all git filters
tokf search "cargo test"     # find cargo test filters
tokf search "" -n 50         # list 50 most popular filters
tokf search git --json       # machine-readable output
```

---

## Installing a Filter

```sh
tokf install <filter>
```

`<filter>` can be:

- A **command pattern** substring — tokf searches the registry and installs the top match.
- A **content hash** (64 hex characters) — installs a specific, pinned version.

On install, tokf:

1. Downloads the filter TOML and any bundled test files.
2. Verifies the content hash to detect tampering.
3. Writes the filter under `~/.config/tokf/filters/` (global) or `.tokf/filters/` (local).
4. Runs the bundled test suite (if any). Rolls back on failure.

### Options

| Flag | Description |
|------|-------------|
| `--local` | Install to project-local `.tokf/filters/` instead of global config |
| `--force` | Overwrite an existing filter at the same path |
| `--dry-run` | Preview what would be installed without writing any files |

### Examples

```sh
tokf install git push                  # install top result for "git push"
tokf install git push --local          # install into current project only
tokf install git push --dry-run        # preview the install
tokf install <64-hex-hash> --force     # install a pinned version, overwriting existing
```

### Attribution

Installed filters include an attribution header at the top of the TOML:

```toml
# Published by @alice · hash: <hash> · https://tokf.net/filters/<hash>
```

This header is stripped automatically when the filter is loaded.

### Security

> **Warning:** Community filters are third-party code. Review a filter at
> `https://tokf.net/filters/<hash>` before installing it in production environments.

tokf verifies the content hash of every downloaded filter to detect server-side tampering.
Test filenames are validated to prevent path traversal attacks.

---

## Publishing a Filter

See [Publishing Filters](./publishing-filters.md) for how to share your own filters.

---

## Server authentication API

tokf-server uses the [GitHub device flow](https://docs.github.com/en/apps/oauth-apps/building-oauth-apps/authorizing-oauth-apps#device-flow) so CLI clients can authenticate without handling secrets.

### `POST /api/auth/device`

Starts the device authorization flow. Returns a `user_code` and `verification_uri` for the user to visit in their browser. Rate-limited to 10 requests per IP per hour.

**Response (201 Created):**

```json
{
  "device_code": "dc-abc123",
  "user_code": "ABCD-1234",
  "verification_uri": "https://github.com/login/device",
  "expires_in": 900,
  "interval": 5
}
```

### `POST /api/auth/token`

Polls for a completed device authorization. The CLI calls this on an interval until the user has authorized.

**Request body:**

```json
{ "device_code": "dc-abc123" }
```

**Response (200 OK) when authorized:**

```json
{
  "access_token": "...",
  "token_type": "bearer",
  "expires_in": 7776000,
  "user": { "id": 1, "username": "octocat", "avatar_url": "..." }
}
```

**Response (200 OK) while waiting:**

```json
{ "error": "authorization_pending" }
```

Re-polling a completed device code is idempotent — a fresh token is issued.

### Environment variables

| Variable | Required | Description |
|---|---|---|
| `GITHUB_CLIENT_ID` | yes | OAuth App client ID |
| `GITHUB_CLIENT_SECRET` | yes | OAuth App client secret |
| `TRUST_PROXY` | no | Set `true` to trust `X-Forwarded-For` for IP extraction (default `false`) |

---

## Acknowledgements

tokf was heavily inspired by [rtk](https://github.com/rtk-ai/rtk) ([rtk-ai.app](https://www.rtk-ai.app/)) — a CLI proxy that compresses command output before it reaches an AI agent's context window. rtk pioneered the idea and demonstrated that 60–90% context reduction is achievable across common dev tools. tokf takes a different approach (TOML-driven filters, user-overridable library, Claude Code hook integration) but the core insight is theirs.

---

## License

MIT — see [LICENSE](LICENSE).
