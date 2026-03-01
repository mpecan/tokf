---
title: Writing Filters
description: Author TOML filter files to shape any command's output.
order: 2
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
