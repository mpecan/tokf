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
tokf verify --list --require-all  # show ✓/✗ coverage per filter
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

### Piped commands

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
| `git/status` | `git status` |
| `cargo/build` | `cargo build` |
| `cargo/check` | `cargo check` |
| `cargo/clippy` | `cargo clippy` |
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

## Writing a filter

Filters are TOML files placed in `.tokf/filters/` (project-local) or `~/.config/tokf/filters/` (user-level). Project-local filters take priority over user-level, which take priority over the built-in library.

### Minimal example

```toml
command = "my-tool"

[on_success]
output = "ok ✓"

[on_failure]
tail = 10
```

### Command matching

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

### Common fields

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

### Writing test cases

Filter tests live in a `<stem>_test/` directory adjacent to the filter TOML:

```
filters/
  git/
    push.toml          ← filter config
    push_test/         ← test suite
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

### Template pipes

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

### Lua escape hatch

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

Available globals: `output` (string), `exit_code` (integer), `args` (table).
Return a string to replace output, or `nil` to fall through to the rest of the TOML pipeline.
The sandbox blocks `io`, `os`, and `package` — no filesystem or network access from scripts.

### Filter variants

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

---

## Filter resolution

1. `.tokf/filters/` in the current directory (repo-local overrides)
2. `~/.config/tokf/filters/` (user-level overrides)
3. Built-in library (embedded in the binary)

First match wins. Use `tokf which "git push"` to see which filter would activate.

---

## Token savings tracking

tokf records input/output byte counts per run in a local SQLite database:

```sh
tokf gain              # summary: total bytes saved and reduction %
tokf gain --daily      # day-by-day breakdown
tokf gain --by-filter  # breakdown by filter
tokf gain --json       # machine-readable output
```

---

## Output history

tokf records raw and filtered outputs in a local SQLite database, useful for debugging filters or reviewing what an AI agent saw:

```sh
tokf history list              # recent entries (current project)
tokf history list -l 20        # show 20 entries
tokf history list --all        # entries from all projects
tokf history show 42           # full details for entry #42
tokf history search "error"    # search by command or output content
tokf history clear             # clear current project history
tokf history clear --all       # clear all history (destructive)
```

### History hint

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
Filtered - for full content call: `tokf history show 99`
```

The hint is appended to stdout so it is visible to both humans and LLMs in the tool output. The history entry itself always stores the clean filtered output, without the hint line.

---

## OpenTelemetry metrics export

tokf can export token-compression metrics to any OTLP-compatible backend (Datadog,
Grafana, Honeycomb, New Relic, Jaeger, etc.).  This requires building with
`--features otel`:

```sh
cargo install tokf --features otel
```

Enable at runtime:

```sh
export TOKF_TELEMETRY_ENABLED=true
export OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4318
tokf run cargo build
```

Or pass `--otel-export` to force a one-shot export without changing env vars:

```sh
tokf --otel-export run cargo build
```

**Configuration** (env vars override `~/.config/tokf/config.toml`):

| Variable | Default | Description |
|----------|---------|-------------|
| `TOKF_TELEMETRY_ENABLED` | `false` | `true`, `1`, or `yes` to enable |
| `OTEL_EXPORTER_OTLP_ENDPOINT` | `http://localhost:4318` | OTLP collector endpoint |
| `OTEL_EXPORTER_OTLP_PROTOCOL` | `http` | `http` or `grpc` (grpc needs `--features otel-grpc`) |
| `OTEL_EXPORTER_OTLP_HEADERS` | (none) | Comma-separated `key=value` auth headers |
| `OTEL_RESOURCE_ATTRIBUTES` | (none) | Resource attrs; `service.name` is extracted |
| `TOKF_OTEL_PIPELINE` | (none) | Pipeline label added to every metric (`tokf.pipeline`) |

**Metrics emitted per `tokf run` invocation:**

| Metric | Description |
|--------|-------------|
| `gen_ai.client.token.usage` | Estimated tokens (split by `gen_ai.token.type`: `input`/`output`) |
| `tokf.filter.invocations` | Number of invocations |
| `tokf.filter.input_lines` / `output_lines` / `lines_removed` | Line counts |
| `tokf.compression.ratio` | output ÷ input lines |
| `tokf.tokens.saved` | Estimated tokens saved |
| `tokf.filter.duration` | Filter wall-clock time (seconds) |

Token counts are **estimates** (`bytes ÷ 4`) — not exact tokenizer output.

Export is **best-effort**: tokf waits at most 200 ms for the OTLP flush. Every
invocation is also recorded to a local SQLite database; a future `tokf telemetry sync`
command (#89) will replay any unsynced events.

---

## Cache management

tokf caches the filter discovery index for faster startup. The cache rebuilds automatically when filters change, but you can manage it manually:

```sh
tokf cache info    # show cache location, size, and validity
tokf cache clear   # delete the cache, forcing a rebuild on next run
```

---

## Acknowledgements

tokf was heavily inspired by [rtk](https://github.com/rtk-ai/rtk) ([rtk-ai.app](https://www.rtk-ai.app/)) — a CLI proxy that compresses command output before it reaches an AI agent's context window. rtk pioneered the idea and demonstrated that 60–90% context reduction is achievable across common dev tools. tokf takes a different approach (TOML-driven filters, user-overridable library, Claude Code hook integration) but the core insight is theirs.

---

## License

MIT — see [LICENSE](LICENSE).
