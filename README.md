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

## Set up automatic filtering

If you use an AI coding tool, install the hook so every command is filtered automatically — no `tokf run` prefix needed:

```sh
# Claude Code (recommended: --global so it works in every project)
tokf hook install --global

# OpenCode
tokf hook install --tool opencode --global

# OpenAI Codex CLI
tokf hook install --tool codex --global
```

Drop `--global` to install for the current project only. See [Claude Code hook](#claude-code-hook) for details on each tool, the `--path` flag, and optional extras like the filter-authoring skill.

---

## Usage

### Run a command with filtering

```sh
tokf run git push origin main
tokf run cargo test
tokf run docker build .
```

### Apply a filter to a fixture

```sh
tokf apply filters/git/push.toml tests/fixtures/git_push_success.txt --exit-code 0
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
tokf verify --safety           # run safety checks (prompt injection, shell injection, hidden unicode)
tokf verify git/push --safety  # safety check a specific filter
```

### Task runner filtering

tokf automatically wraps `make` and `just` so that each recipe line is individually filtered:

```sh
make check    # each recipe line (cargo test, cargo clippy, ...) is filtered
just test     # same — each recipe runs through tokf
```

See [Rewrite configuration](#rewrite-configuration-rewritestoml) for details and customization.

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
- Empty output: `[tokf err] no errors detected (empty output)`
- Short output (< 10 lines): shows `[tokf err]` header with full output
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

### Using generic commands with rewrites

Generic commands can be integrated with the [rewrite system](rewrites-config.md) so they trigger automatically through the hook. Add rules to `.tokf/rewrites.toml` for commands that don't have dedicated filters:

```toml
# Route build commands without filters through tokf err
[[rewrite]]
match = "^mix compile"
replace = "tokf err {0}"

[[rewrite]]
match = "^cmake --build"
replace = "tokf err {0}"

# Route test runners without filters through tokf test
[[rewrite]]
match = "^mix test"
replace = "tokf test {0}"

[[rewrite]]
match = "^ctest"
replace = "tokf test {0}"

# Summarize long-running commands
[[rewrite]]
match = "^terraform plan"
replace = "tokf summary {0}"
```

**Important:** User rewrite rules are checked *before* filter matching. Don't add rules for commands that already have dedicated filters (like `cargo build`, `npm test`) — the dedicated filter will produce better output than the generic command.

To check whether a command already has a filter: `tokf which "cargo build"`.

### Tracking and history

Generic commands record to the same tracking database and history as `tokf run`, using filter names `_builtin/err`, `_builtin/test`, and `_builtin/summary`. Use `tokf raw last` to see the full uncompressed output.

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
description = "Compact git push output"  # human-readable description (shown in `tokf ls`)

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
truncate_lines_at = 120       # truncate lines longer than N chars (with trailing …)

tail = 30                     # keep last N lines regardless of exit code (branch tail overrides)
on_empty = "git push: ok"     # message when filter produces empty output (all lines stripped)

show_history_hint = true      # append a hint line (`tokf raw <id>`) pointing to the full output in history
inject_path = true            # inject shims into PATH so sub-processes (e.g. git hooks) are filtered

passthrough_args = ["--watch", "--web", "-w"]  # skip filter when user passes these flags

# Lua escape hatch — for logic TOML can't express (see Lua Escape Hatch section)
[lua_script]
lang = "luau"
source = 'return output:upper()'    # inline script
# file = "transform.luau"           # or reference a local file (auto-inlined on publish)

match_output = [              # whole-output substring checks, short-circuit the pipeline
  { contains = "rejected", output = "push rejected" },
]

[on_success]                  # branch for exit code 0
output = "ok ✓ {2}"          # template; {output} = pre-filtered output

[on_failure]                  # branch for non-zero exit
tail = 10                     # keep the last N lines (overrides top-level tail)
```

## Passthrough args

Some filters inject flags like `--json` or `--format` via the `run` field. When users pass conflicting flags (e.g. `--watch`), the combined command fails. The `passthrough_args` field declares flag prefixes that trigger passthrough mode — tokf skips the filter entirely and runs the original command as-is.

```toml
command = "gh pr checks *"
run = "gh pr checks {args} --json name,state,workflow"
passthrough_args = ["--watch", "--web", "-w"]
```

**Matching semantics**: each user arg is checked with `starts_with` against each prefix. This handles `--format=table` matching `--format`, while `-w` does **not** match `--watch` (correct — they are different flags). Short-flag prefixes like `-o` also match concatenated forms like `-oyaml` (common in tools like `kubectl`). Empty-string prefixes are ignored. When any arg matches, no `run` override is applied and no filter pipeline runs.

**Variant interaction**: passthrough is checked on the resolved filter config after file-based variant detection. If a parent filter delegates to a variant via file detection, the variant's own `passthrough_args` apply. Output-pattern variants (post-execution) are not resolved when passthrough is active.

Use `--verbose` to see when passthrough activates:

```
$ tokf run gh pr checks 142 --watch --verbose
[tokf] passthrough: user args match passthrough_args, skipping filter
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

## Sections

Sections collect lines into named buckets using a state-machine model. They are processed on the raw output (before skip/keep filtering) so structural markers like blank lines are available.

```toml
[[section]]
name = "failures"
enter = "^failures:$"        # regex that starts collecting
exit = "^failures:$"         # regex that stops collecting (second occurrence)
split_on = "^\\s*$"          # split collected lines into blocks at blank lines
collect_as = "failure_blocks" # name used in templates: {failure_blocks}

[[section]]
name = "summary"
match = "^test result:"      # stateless: collect any matching line
collect_as = "summary_lines"
```

**Stateful sections** (with `enter`/`exit`) toggle on/off as the state machine hits the enter/exit patterns. **Stateless sections** (with `match` only) collect every matching line regardless of state.

Section data is available in templates:
- `{failure_blocks}` — the collected items
- `{failure_blocks.count}` — number of items (blocks if `split_on` is set, otherwise lines)
- `{failure_blocks | each: "..." | join: "\\n"}` — iterate over items

## Aggregates

Aggregates extract numeric values from section items and produce named variables for templates.

**Single aggregate** (backwards compatible):

```toml
[on_success]
output = "{passed} passed ({suites} suites)"

[on_success.aggregate]
from = "summary_lines"
pattern = 'ok\. (\d+) passed'
sum = "passed"
count_as = "suites"
```

**Multiple aggregates** — use `[[on_success.aggregates]]` (plural) to define several rules:

```toml
[on_success]
output = "✓ {passed} passed, {failed} failed, {ignored} ignored ({suites} suites)"

[[on_success.aggregates]]
from = "summary_lines"
pattern = 'ok\. (\d+) passed'
sum = "passed"
count_as = "suites"

[[on_success.aggregates]]
from = "summary_lines"
pattern = '(\d+) failed'
sum = "failed"

[[on_success.aggregates]]
from = "summary_lines"
pattern = '(\d+) ignored'
sum = "ignored"
```

Each rule scans the named section's items. `sum` accumulates the first capture group as a number. `count_as` counts the number of matching lines. Both singular `aggregate` and plural `aggregates` can be used together — they are merged at runtime.

## Chunk processing

Chunks split raw output into repeating structural blocks, extract structured data per-block, and produce named collections for template rendering. Use chunks when you need per-block breakdown (e.g., per-crate test results in a Cargo workspace).

> **Note:** Like sections, chunks operate on the raw (unfiltered) command output. Skip/keep patterns do not affect chunk processing. This ensures structural markers are available for splitting.

```toml
[[chunk]]
split_on = "^\\s*Running "   # regex that marks the start of each chunk
include_split_line = true     # include the splitting line in the chunk (default: true)
collect_as = "suites_detail"  # name for the structured collection
group_by = "crate_name"       # merge chunks sharing this field value

[chunk.extract]
pattern = 'deps/([\w_-]+)-'  # extract a field from the split (header) line
as = "crate_name"

[[chunk.aggregate]]
pattern = '(\d+) passed'     # aggregates run within each chunk's own lines
sum = "passed"

[[chunk.aggregate]]
pattern = '(\d+) failed'
sum = "failed"

[[chunk.aggregate]]
pattern = '^test result:'
count_as = "suite_count"
```

**Fields**:

| Field | Description |
|---|---|
| `split_on` | Regex marking the start of each chunk |
| `include_split_line` | Whether the splitting line is part of the chunk (default: `true`) |
| `collect_as` | Name for the resulting structured collection |
| `extract` | Extract a named field from the header line (`pattern` + `as`) |
| `body_extract` | Extract fields from body lines (`pattern` + `as`, first match wins) |
| `aggregate` | Per-chunk aggregation rules (run within each chunk's own lines) |
| `group_by` | Merge chunks sharing the same field value, summing numeric fields |
| `children_as` | When set with `group_by`, preserve original items as a nested collection under this name |
| `carry_forward` | On `extract` or `body_extract`: inherit value from the previous chunk when the pattern doesn't match |

The resulting structured collection is available in templates as `{suites_detail}` and supports field access in `each` pipes.

### Structured collections in templates

When a chunk produces a structured collection, each item has named fields. Use `each` to iterate with field access:

```toml
[on_success]
output = """✓ cargo test: {passed} passed ({suites} suites)
{suites_detail | each: "  {crate_name}: {passed} passed ({suite_count} suites)" | join: "\\n"}"""
```

Inside the `each` template, all named fields from the chunk item are available as variables (`{crate_name}`, `{passed}`, `{suite_count}`), plus `{index}` (1-based) and `{value}` (debug representation).

`{suites_detail.count}` returns the number of items in the collection.

### Carry-forward fields

When a chunk's `extract` or `body_extract` rule has `carry_forward = true`, chunks that don't match the pattern inherit the value from the most recent chunk that did. This is useful when boundary markers (like `Running unittests`) identify a group, and subsequent chunks (like integration test suites) should inherit that identity.

```toml
[chunk.extract]
pattern = 'unittests.+deps/([\w_-]+)-'
as = "crate_name"
carry_forward = true
```

### Tree-structured groups (children_as)

When `children_as` is set alongside `group_by`, the grouped collection preserves each group's original items as a nested collection. Inside an `each` template, the children are accessible by the `children_as` name and support their own `each`/`join` pipes:

```toml
[[chunk]]
split_on = "^\\s*Running "
collect_as = "suites_detail"
group_by = "crate_name"
children_as = "children"

[on_success]
output = """✓ {passed} passed ({suites} suites)
{suites_detail | each: "  {crate_name}: {passed} passed\n{children | each: \"    {suite_name}: {passed}\" | join: \"\\n\"}" | join: "\\n"}"""
```

This produces tree output like:

```
✓ 565 passed (2 suites)
  tokf: 565 passed
    unittests src/lib.rs: 550
    tests/cli_basic.rs: 15
```

## JSON extraction

When commands produce JSON output (e.g. `kubectl get pods -o json`, `gh api`, `docker inspect`), use the `[json]` block to extract values via `JSONPath` (RFC 9535) instead of line-based parsing.

```toml
command = "kubectl get pods -o json"

[json]

# Array of objects → structured collection (usable with |each: pipe)
# Auto-generates {pods_count} with the number of matched items.
[[json.extract]]
path = "$.items[*]"
as = "pods"

# Sub-field extraction from each matched object (dot-path, not JSONPath)
[[json.extract.fields]]
field = "metadata.name"
as = "name"

[[json.extract.fields]]
field = "status.phase"
as = "phase"

[on_success]
output = "Pods ({pods_count}):\n{pods | each: \"  {name}: {phase}\" | join: \"\\n\"}"
```

**Result mapping**:

| JSONPath result | Behavior |
|---|---|
| Single scalar (string/number/bool/null) | `vars["as_name"] = string_value` |
| Array of scalars | `ChunkData::Flat` with `{value}` key per item; auto-generates `{as_name_count}` |
| Array of objects (with `fields`) | `ChunkData::Flat` with named field keys; auto-generates `{as_name_count}` |
| Array of objects (without `fields`) | All top-level scalar fields auto-flattened; auto-generates `{as_name_count}` |

**Pipeline position**: JSON extraction runs after `lua_script` (step 2c) and replaces `parse`/`sections`/`chunks` — when `[json]` is configured, those line-based structural steps are skipped. The extracted vars and chunks flow into branch selection (`on_success`/`on_failure`) and template rendering.

**Dot-path syntax** for `[[json.extract.fields]]`: uses simple dot-separated paths (not JSONPath). Supports array indices: `containers.0.name` traverses `obj["containers"][0]["name"]`.

**Error handling**: if the input is not valid JSON, extraction is skipped and tokf falls back to raw output (templates are not rendered). Invalid JSONPath or dot-path expressions are silently skipped.

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

### Safety checks

Add `--safety` to detect potential security issues in your filter:

```sh
tokf verify --safety
tokf verify my-filter --safety --json
```

Safety checks scan for:

- **Prompt injection** — templates containing patterns like "ignore previous instructions", "you are now", "system prompt", etc. Both static config text and filtered output are checked (NFKC-normalized to handle compatibility/fullwidth forms; cross-script homoglyphs are not fully covered).
- **Shell injection** — `run`, `step[].run`, and rewrite replacement strings containing shell metacharacters (`$(...)`, backticks, `;`, `&&`, pipes, redirections). Known-safe templates like `tokf run {0}` are allowlisted.
- **Hidden Unicode** — zero-width spaces, RTL overrides, and other invisible characters that could smuggle content.

Safety warnings do **not** block publishing — filters with issues are published with `safety_passed = false` and the registry shows a warning badge. Use `--safety` locally to catch issues before publishing.

---


## Configuration files

tokf uses TOML files for configuration. Files can live at two levels:

| File | Global path | Project-local path | Purpose |
|------|-------------|---------------------|---------|
| `config.toml` | `~/.config/tokf/config.toml` | `.tokf/config.toml` | History retention, sync settings, telemetry |
| `rewrites.toml` | `~/.config/tokf/rewrites.toml` | `.tokf/rewrites.toml` | Shell rewrite rules |
| `auth.toml` | `~/.config/tokf/auth.toml` | — | Registry authentication (managed by `tokf auth`) |
| `machine.toml` | `~/.config/tokf/machine.toml` | — | Machine UUID for remote sync |

> Paths shown are for Linux/macOS. On macOS, the global directory is `~/Library/Application Support/tokf`. On Windows, it is `%APPDATA%/tokf`.

Set `TOKF_HOME` to redirect **all** user-level paths to a single directory (like `CARGO_HOME` or `RUSTUP_HOME`):

```sh
export TOKF_HOME=/opt/tokf   # all config, data, and cache under /opt/tokf
```

Run `tokf info` to see which paths are active.

---

## config.toml

### `[history]`

Controls how many filtered outputs are retained in the local history database.

```toml
[history]
retention = 10   # number of history entries to keep (default: 10)
```

### `[sync]`

Settings for remote filter-usage sync (requires `tokf auth login`).

```toml
[sync]
auto_sync_threshold = 100   # sync after this many unsynced records (default: 100)
upload_usage_stats = true   # upload anonymous usage statistics (default: not set)
```

### `[shims]`

Controls PATH-based shim injection for sub-process filtering. When filters use `inject_path = true`, tokf generates shim scripts and prepends them to `PATH` so that sub-processes (e.g. commands inside git hooks) are automatically filtered.

```toml
[shims]
enabled = true   # generate and use shims for inject_path filters (default: true)
```

Set to `false` to disable shim generation and PATH injection globally. This overrides any per-filter `inject_path = true` setting — no shims will be generated or used.

```sh
tokf config set shims.enabled false
```

Shim scripts are stored in `~/.cache/tokf/shims/` (or `$TOKF_HOME/shims/`). Disabling shims via `config set` immediately removes any existing shim scripts.

> **Note:** `shims.enabled` is read from the global config only — project-local overrides are not checked, to avoid filesystem scanning on every command invocation.

### `[telemetry]`

Export metrics via OpenTelemetry OTLP. Disabled by default.

```toml
[telemetry]
enabled = true
endpoint = "http://localhost:4318"   # OTLP collector endpoint
protocol = "http"                    # "http" (default) or "grpc"
service_name = "tokf"                # service.name resource attribute

[telemetry.headers]
x-api-key = "your-secret"
```

Default endpoints by protocol: HTTP uses port `4318`, gRPC uses port `4317`.

Environment variables override config-file values for telemetry — see the [Environment variables](#environment-variables) section below.

### Priority order

For all `config.toml` settings:

1. **Project-local** `.tokf/config.toml` (highest priority)
2. **Global** `~/.config/tokf/config.toml`
3. **Built-in defaults**

### Managing config via CLI

```sh
tokf config show              # show all effective config with source paths
tokf config show --json       # machine-readable JSON output
tokf config get <key>         # print a single value (for scripting)
tokf config set <key> <value> # set a value in the global config
tokf config set --local <key> <value>  # set in project-local .tokf/config.toml
tokf config print             # print raw config file contents
tokf config path              # show config file paths with existence status
```

Available keys: `history.retention`, `shims.enabled`, `sync.auto_sync_threshold`, `sync.upload_stats`.

---

## rewrites.toml

Rewrite rules let tokf intercept and transform commands before execution — wrapping task runners, stripping pipes, and injecting baselines. See the [Rewrite Configuration](#rewrite-configuration-rewritestoml) section for the full reference.

---

## Environment variables

| Variable | Description | Default |
|----------|-------------|---------|
| **Paths** | | |
| `TOKF_HOME` | Redirect all user-level tokf paths (config, data, cache) to a single directory | Platform config dir |
| `TOKF_DB_PATH` | Override the tracking database path only (takes precedence over `TOKF_HOME`) | Platform data dir |
| **Runtime** | | |
| `TOKF_DEBUG` | Enable debug output (set to `1` or `true`) | unset |
| `TOKF_NO_FILTER` | Skip filtering in shell mode (set to `1`, `true`, or `yes`) | unset |
| `TOKF_VERBOSE` | Print filter resolution details in shell mode | unset |
| `TOKF_PRESERVE_COLOR` | Preserve ANSI color codes in filtered output | unset |
| `TOKF_HTTP_TIMEOUT` | HTTP request timeout in seconds (for remote operations) | `5` |
| `NO_COLOR` | Disable colored output in `tokf gain` (per [no-color.org](https://no-color.org/)) | unset |
| **Telemetry** | | |
| `TOKF_TELEMETRY_ENABLED` | Enable telemetry export (`true`, `1`, or `yes`) — overrides config file | unset |
| `TOKF_OTEL_PIPELINE` | Pipeline label attached to telemetry metrics | unset |
| `OTEL_EXPORTER_OTLP_ENDPOINT` | OTLP collector endpoint — overrides config file | `http://localhost:4318` |
| `OTEL_EXPORTER_OTLP_PROTOCOL` | `http` or `grpc` — overrides config file | `http` |
| `OTEL_EXPORTER_OTLP_HEADERS` | Comma-separated `key=value` header pairs — overrides config file | empty |
| `OTEL_RESOURCE_ATTRIBUTES` | Comma-separated `key=value` resource attributes; `service.name` is extracted | empty |
| **Registry (CI)** | | |
| `TOKF_REGISTRY_URL` | Registry base URL for `tokf regenerate-examples` | — |
| `TOKF_SERVICE_TOKEN` | Service token for registry authentication | — |

---

## CLI flags

### Global flags

Available on all `tokf run` invocations and most subcommands:

| Flag | Description |
|------|-------------|
| `--timing` | Print how long filtering took |
| `--verbose` | Show filter resolution details |
| `--no-filter` | Pass output through without filtering |
| `--no-cache` | Bypass the binary filter discovery cache |
| `--no-mask-exit-code` | Propagate real exit code instead of masking to 0 |
| `--preserve-color` | Preserve ANSI color codes in filtered output |
| `--otel-export` | Export metrics via OpenTelemetry OTLP for this invocation |

### Run-specific flags

| Flag | Description |
|------|-------------|
| `--baseline-pipe <cmd>` | Pipe command for fair baseline accounting (injected by rewrite rules) |
| `--prefer-less` | Compare filtered vs piped output and use whichever is smaller |

---

## Filter resolution order

tokf searches for matching filters in three tiers, stopping at the first match:

1. **Project-local** — `.tokf/filters/` in the project root
2. **User-level** — `~/.config/tokf/filters/` (or `$TOKF_HOME/filters/`)
3. **Standard library** — built-in filters shipped with tokf

To override a built-in filter, eject it to your project or user directory:

```sh
tokf eject cargo/test              # copy to .tokf/filters/ (project-local)
tokf eject --global cargo/test     # copy to ~/.config/tokf/filters/ (user-level)
```

The ejected copy takes priority on subsequent runs. See `tokf eject --help` for details.

---

## Directory layout

```
~/.config/tokf/                    # global config directory ($TOKF_HOME overrides)
├── config.toml                    # history, sync, telemetry settings
├── rewrites.toml                  # shell rewrite rules
├── auth.toml                      # registry credentials (managed by tokf auth)
├── machine.toml                   # machine UUID for remote sync
└── filters/                       # user-level filter overrides
    └── cargo/
        └── test.toml

~/.local/share/tokf/               # data directory
└── tracking.db                    # token savings database ($TOKF_DB_PATH overrides)

~/.cache/tokf/                     # cache directory
├── manifest.bin                   # binary filter discovery cache
└── shims/                         # generated shim scripts for inject_path

<project>/
└── .tokf/                         # project-local overrides
    ├── config.toml                # project-specific settings
    ├── rewrites.toml              # project-specific rewrite rules
    └── filters/                   # project-specific filters
        └── custom/
            └── lint.toml
```

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

### Sandbox

All Lua execution is sandboxed — both in the CLI and on the server:

- **Blocked libraries:** `io`, `os`, `package` — no filesystem or network access.
- **Instruction limit:** 1 million VM instructions (prevents infinite loops).
- **Memory limit:** 16 MB (prevents memory exhaustion).

Scripts that exceed these limits are terminated and treated as a passthrough (the TOML pipeline continues as if no Lua script was configured).

### External script files

For local development you can keep the script in a separate `.luau` file:

```toml
[lua_script]
lang = "luau"
file = "transform.luau"
```

Only one of `file` or `source` may be set — not both. When you run `tokf publish`, file references are automatically inlined (the file content is embedded as `source`) so the published filter is self-contained. The script file must reside within the filter's directory — path traversal (e.g. `../secret.txt`) is rejected.

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

The `🗜️` prefix appears on all filtered output (disable with `tokf config set output.show_indicator false` or `TOKF_SHOW_INDICATOR=false`). The hint line is appended to stdout so it is visible to both humans and LLMs in the tool output. The history entry itself always stores the clean filtered output, without the hint line or indicator.

## Context injection

During `tokf hook install`, tokf creates a `.claude/TOKF.md` file and adds an `@TOKF.md` reference to `.claude/CLAUDE.md`. This gives LLMs a two-line context explaining what `🗜️` means and how to retrieve full output (`tokf raw last`). Use `--no-context` to skip this step.

---


## Setup

```sh
tokf auth login            # authenticate via GitHub device flow
tokf remote setup          # register this machine
tokf sync                  # upload pending usage events
tokf gain --remote         # view aggregate savings across all machines
```

## Authentication

tokf uses the [GitHub device flow](https://docs.github.com/en/apps/oauth-apps/building-oauth-apps/authorizing-oauth-apps#device-flow) so no secrets are handled locally. Tokens are stored in your OS keyring (Keychain on macOS, Secret Service on Linux, Credential Manager on Windows).

```sh
tokf auth login    # start device flow — prints a one-time code, opens browser
tokf auth status   # show current login state and server URL
tokf auth logout   # remove stored credentials
```

## Machine registration

Each machine gets a UUID that links usage events to a physical device. Registration is idempotent — running it again re-syncs the existing record.

```sh
tokf remote setup    # register this machine with the server
tokf remote status   # show local machine ID and hostname (no network call)
```

Machine config is stored in `~/.config/tokf/machine.toml` (or `$TOKF_HOME/machine.toml`).

## Syncing usage data

`tokf sync` uploads pending local usage events to the remote server. Events are deduplicated by cursor — re-syncing the same events is safe.

```sh
tokf sync              # upload pending events
tokf sync --status     # show last sync time and pending event count (no network call)
```

A file lock prevents concurrent syncs. Both `tokf auth login` and `tokf remote setup` must be completed before syncing.

## Viewing remote gain

View aggregate token savings across all your registered machines:

```sh
tokf gain --remote              # summary: total runs, tokens saved, reduction %
tokf gain --remote --by-filter  # breakdown by filter
tokf gain --remote --json       # machine-readable output
```

> **Note:** `--daily` is not available with `--remote`. Use local `tokf gain --daily` for day-by-day breakdowns.

## Backfill

Usage events recorded before hash-based tracking was added may be missing filter hashes. Backfill resolves them from currently installed filters:

```sh
tokf remote backfill             # update events with missing hashes
tokf remote backfill --no-cache  # skip binary config cache during discovery
```

Backfill runs locally — no network call required.

---

For discovering and installing community filters, see [Community Filters](#community-filters). To publish your own, see [Publishing Filters](#publishing-filters). For the full server API reference, see [`docs/reference/api.md`](docs/reference/api.md).

---


## Claude Code hook

tokf integrates with [Claude Code](https://claude.ai/code) as a `PreToolUse` hook that **automatically filters every `Bash` tool call** — no changes to your workflow required.

```sh
tokf hook install          # project-local (.tokf/)
tokf hook install --global # user-level (~/.config/tokf/)
```

Once installed, every command Claude runs through the Bash tool is filtered transparently. Track cumulative savings with `tokf gain`.

### Custom binary path

By default the generated hook script calls bare `tokf`, relying on PATH at runtime. If `tokf` isn't on PATH in the hook's execution environment (common with Linuxbrew or `cargo install` when PATH is only set in interactive shell profiles), pass `--path` to embed a specific binary location:

```sh
tokf hook install --global --path ~/.cargo/bin/tokf
tokf hook install --tool opencode --path /home/linuxbrew/.linuxbrew/bin/tokf
```

### Multiple hooks and permission mode

When using multiple `PreToolUse` hooks (e.g., tokf for filtering + another hook for validation), the default behavior can block subsequent hooks. By default, tokf returns `permissionDecision: "allow"` which tells Claude Code to skip further permission checks.

If you're using another hook that needs to validate commands (like a security policy checker), use `--permission preserve` to omit the permission decision:

```sh
# Edit the generated hook script to use preserve mode
exec tokf hook handle --permission preserve
```

Or install with a custom hook script that includes this flag. The available modes are:

- `--permission allow` (default) - Returns `permissionDecision: "allow"`. Use when tokf is the only hook.
- `--permission preserve` - Omits `permissionDecision`. Allows subsequent hooks to validate permissions.

With `preserve` mode, tokf rewrites the command but lets other hooks decide whether to allow, deny, or ask for confirmation.

tokf also ships a filter-authoring skill that teaches Claude the complete filter schema:

```sh
tokf skill install          # project-local (.claude/skills/)
tokf skill install --global # user-level (~/.claude/skills/)
```

## Gemini CLI

tokf integrates with [Gemini CLI](https://github.com/google-gemini/gemini-cli) as a `BeforeTool` hook that automatically filters `run_shell_command` tool calls.

```sh
tokf hook install --tool gemini-cli          # project-local (.gemini/)
tokf hook install --tool gemini-cli --global # user-level (~/.gemini/)
```

This registers a hook shim in `.gemini/settings.json` (or `~/.gemini/settings.json` for `--global`). When `--no-context` is not set, it also creates `.gemini/TOKF.md` and patches `.gemini/GEMINI.md` with context about the compression indicator.

## Cursor

tokf integrates with [Cursor](https://cursor.com) via a `beforeShellExecution` hook that automatically filters shell commands.

```sh
tokf hook install --tool cursor          # project-local (.cursor/)
tokf hook install --tool cursor --global # user-level (~/.cursor/)
```

This registers a hook in `.cursor/hooks.json` (or `~/.cursor/hooks.json` for `--global`). When `--no-context` is not set, it also creates `.cursor/rules/TOKF.md` with context about the compression indicator.

## Cline

tokf integrates with [Cline](https://cline.bot) via a rules file that instructs the agent to prefix supported commands with `tokf run`.

```sh
tokf hook install --tool cline          # project-local (.clinerules/)
tokf hook install --tool cline --global # user-level (~/Documents/Cline/Rules/)
```

This writes `.clinerules/tokf.md` (or `~/Documents/Cline/Rules/tokf.md` for `--global`), which Cline auto-discovers. The rules file uses `alwaysApply: true` frontmatter.

## Windsurf

tokf integrates with [Windsurf](https://windsurf.com) via a rules file.

```sh
tokf hook install --tool windsurf          # project-local (.windsurf/rules/)
tokf hook install --tool windsurf --global # user-level (appends to global rules)
```

Project-local creates `.windsurf/rules/tokf.md`. Global mode appends a tokf section (with `<!-- tokf:start/end -->` markers for idempotent updates) to `~/.codeium/windsurf/memories/global_rules.md`.

## GitHub Copilot

tokf integrates with [GitHub Copilot](https://github.com/features/copilot) via instruction files. Copilot only supports repo-level instructions (no `--global` option).

```sh
tokf hook install --tool copilot
```

This creates `.github/instructions/tokf.instructions.md` (with `applyTo: "**"` frontmatter) and appends a tokf section to `.github/copilot-instructions.md`.

## Aider

tokf integrates with [Aider](https://aider.chat) via conventions files.

```sh
tokf hook install --tool aider          # project-local (CONVENTIONS.md)
tokf hook install --tool aider --global # user-level (patches ~/.aider.conf.yml)
```

Project-local appends a tokf section to `CONVENTIONS.md` (which Aider auto-discovers). Global mode writes a conventions file and adds it to `~/.aider.conf.yml`'s `read:` list.

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

## Task runners

tokf also integrates with task runners like `make` and `just` by injecting itself as the task runner's shell. Each recipe line is individually filtered while exit codes propagate correctly. See [Rewrite configuration](#rewrite-configuration-rewritestoml) for details.

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

When invoked as `tokf -c 'command'` (or with combined flags like `-cu`, `-ec`), tokf enters **string mode**. The command string is passed through the rewrite system, which rewrites matching commands to `tokf run --no-mask-exit-code ...`. The rewritten command is then delegated to `sh -c` for execution. If no filter matches, the command is delegated to `sh` unchanged.

When invoked with multiple arguments after `-c` (e.g. `tokf -c git status`), tokf enters **argv mode**. Each argument is shell-escaped and joined into a command string, which is then processed the same way as string mode. This form is used by PATH shims.

Shell mode is not typically invoked directly; it is called by task runners (make, just) and PATH shims.

### Compound and complex recipe lines

Compound commands (`&&`, `||`, `;`) are split at chain operators and each segment is individually rewritten. This means both halves of `git add . && cargo test` can be filtered. Pipes, redirections, and other shell constructs within each segment are handled by the rewrite system's pipe stripping logic (see [Piped commands](#piped-commands)) or passed through to `sh` unchanged.

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

## Routing to generic commands

For commands that don't have a dedicated filter, you can route them through [generic commands](generic-commands.md) (`tokf err`, `tokf test`, `tokf summary`) via rewrite rules:

```toml
# .tokf/rewrites.toml

# Build commands → error extraction
[[rewrite]]
match = "^mix compile"
replace = "tokf err {0}"

# Test runners → failure extraction
[[rewrite]]
match = "^mix test"
replace = "tokf test {0}"

# Long-running commands → heuristic summary
[[rewrite]]
match = "^terraform plan"
replace = "tokf summary {0}"
```

**Note:** User rewrite rules fire *before* filter matching. Only add these for commands that don't already have a filter — check with `tokf which "<command>"`. Commands with dedicated filters (e.g. `cargo build`, `git status`) produce better output through `tokf run`.

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

---


## Overview

The tokf community registry lets you discover filters published by other users, install them
locally, and share your own filters with the community.

Authentication is required for all registry operations. Run `tokf auth login` first.

---

## Searching for Filters

```sh
tokf search <query...>
```

Returns filters whose command pattern matches `<query>` as a substring, ranked by token savings
and install count. Multi-word queries work without quotes:

```sh
tokf search git push         # no quotes needed
tokf search "git push"       # also works
```

### Interactive Mode

When stderr is a terminal, search displays an interactive menu with arrow-key selection.
Choosing a filter flows directly into `tokf install`:

```
> git push [stdlib]  @mpecan  savings:45%  tests:3  runs:12,234
  git push --force   @alice   savings:38%  tests:1  runs:891
  cargo build        @bob     savings:80%  tests:2  runs:500
```

Press Enter to install the selected filter, or Escape to cancel.

### Non-interactive Mode

When stderr is not a terminal (for example, when its output is piped: `tokf search git 2>&1 | cat`), a static table is printed to stderr:

```
COMMAND              AUTHOR    SAVINGS%  TESTS      RUNS
git push             alice       42.3%      3     1,234
git push --force     bob         38.1%      1       891
```

### Options

| Flag | Description |
|------|-------------|
| `-n, --limit <N>` | Maximum results to return (default: 20, max: 100) |
| `--json` | Output raw JSON array to stdout (no interactive UI) |

> **Note:** Flags (`--json`, `-n`) must come **before** the query words.
> `tokf search --json git push` works; `tokf search git push --json` sends `--json` as part of
> the query string.

### Examples

```sh
tokf search git              # find all git filters (interactive if TTY)
tokf search cargo test       # multi-word query, no quotes needed
tokf search -n 50 ""         # list 50 most popular filters
tokf search --json git       # machine-readable JSON output
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

## Updating Test Suites

After publishing a filter, the filter TOML itself is immutable (same content = same hash), but you
can replace the bundled test suite at any time:

```sh
tokf publish --update-tests <filter-name>
```

This replaces the **entire** test suite in the registry with the current local `_test/` directory
contents. Only the original author can update tests.

### Options

| Flag | Description |
|------|-------------|
| `--dry-run` | Preview which test files would be uploaded without making changes |

### Examples

```sh
tokf publish --update-tests git/push            # replace test suite for git/push
tokf publish --update-tests git/push --dry-run  # preview only
```

### Notes

- The filter's identity (content hash) does not change.
- The old test suite is deleted and fully replaced by the new one.
- You must be the original author of the filter.

---

---


## tokf discover

`tokf discover` scans Claude Code session files to find commands that have **no matching tokf filter**, helping you identify where to create new filters for maximum token savings.

By default, commands that already have a matching filter are hidden — if the hook is installed, those are already being filtered. Use `--include-filtered` to see the full picture including commands with existing filters.

```bash
# Scan sessions for the current project
tokf discover

# Also show commands that have existing filters
tokf discover --include-filtered

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
[tokf] 201 commands have filters (use --include-filtered to show)

COMMAND                        FILTER               RUNS     TOKENS
----------------------------------------------------------------------
python manage.py migrate       (none)                 34      12.1k
terraform plan                 (none)                 28       9.8k
helm upgrade                   (none)                 15       6.2k

Total unfiltered output: 28.1k tokens across 443 commands
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
| `--include-filtered` | Also show commands that already have a matching filter |

### How It Works

1. Locates Claude Code session JSONL files in `~/.claude/projects/`
2. Extracts all Bash `tool_use` / `tool_result` pairs
3. Skips commands already wrapped with `tokf run`
4. Matches remaining commands against available tokf filters
5. By default shows only commands with no matching filter
6. Aggregates and ranks by estimated token count

---


## Publishing a Filter

```sh
tokf publish <filter-name>
```

Publishes a local filter to the community registry under the MIT license. Authentication is required — run `tokf auth login` first.

### Requirements

- The filter must be a **user-level or project-local** filter (not a built-in). Use `tokf eject` first if needed.
- At least one **test file** must exist in the adjacent `_test/` directory. The server runs these tests against your filter before accepting the upload.
- You must accept the **MIT license** (prompted on first publish, remembered afterwards).

### What happens on publish

1. The filter TOML is read and validated.
2. If the filter uses `lua_script.file`, the referenced script is **automatically inlined** — its content is embedded as `lua_script.source` so the published filter is self-contained. The script file must reside within the filter's directory (path traversal is rejected).
3. A content hash is computed from the parsed config. This hash is the filter's permanent identity.
4. The filter and test files are uploaded. The server verifies tests pass before accepting.
5. On success, the registry URL is printed.

### Options

| Flag | Description |
|------|-------------|
| `--dry-run` | Preview what would be published without uploading |
| `--update-tests` | Replace the test suite for an already-published filter |

### Examples

```sh
tokf publish git/push                  # publish a filter
tokf publish git/push --dry-run        # preview only
tokf publish --update-tests git/push   # replace test suite
```

### Size limits

- Filter TOML: 64 KB max
- Total upload (filter + tests): 1 MB max

### Lua scripts in published filters

Published filters must use **inline `source`** for Lua scripts — `lua_script.file` is not supported on the server. The `tokf publish` command handles this automatically by reading the file and embedding its content. You don't need to change your filter.

All Lua scripts in published filters are executed in a sandbox with resource limits (1 million instructions, 16 MB memory) during server-side test verification.

---

## Server API

For the full API reference (all endpoints, request/response shapes, rate limits, and environment variables), see [`docs/reference/api.md`](docs/reference/api.md). For deployment instructions, see [`DEPLOY.md`](DEPLOY.md).

---

## Acknowledgements

tokf was heavily inspired by [rtk](https://github.com/rtk-ai/rtk) ([rtk-ai.app](https://www.rtk-ai.app/)) — a CLI proxy that compresses command output before it reaches an AI agent's context window. rtk pioneered the idea and demonstrated that 60–90% context reduction is achievable across common dev tools. tokf takes a different approach (TOML-driven filters, user-overridable library, Claude Code hook integration) but the core insight is theirs.

---

## License

MIT — see [LICENSE](LICENSE).
