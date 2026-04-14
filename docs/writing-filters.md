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

**Variant interaction**: passthrough is checked on the resolved filter config after file-based and args-based variant detection. If a parent filter delegates to a variant (via file detection or `args_pattern`), the variant's own `passthrough_args` apply — not the parent's. Output-pattern variants (post-execution) are not resolved when passthrough is active.

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

## Tree restructuring

When a filter emits a list of file paths, common directory prefixes are repeated on every line. The `[tree]` section restructures the output into a directory tree, writing each shared prefix once. Reusable across any path-shaped filter (`git status`, `git diff --name-only`, etc.).

```toml
command = "git status"

[tree]
# Regex with two capture groups: (1) decoration to keep on the leaf
# (e.g. "M  ", "?? "), (2) the path itself.
pattern = '^(.. )(.+)$'

# Lines that don't match (e.g. "main [synced]" branch headers) are kept
# verbatim: unmatched lines before the first matched path stay in place
# above the tree, and any later unmatched lines are emitted after the
# tree. Set to false to drop them.
passthrough_unmatched = true

# Engagement gates — when not satisfied, the original flat output is
# returned unchanged. Tuned per filter.
min_files = 3            # require at least N matched paths
min_shared_depth = 1     # require at least N common directory levels

# Visual style. "indent" is the cheapest in token count (plain 2-space
# indent, no connectors). "unicode" uses ├─ │ └─ box-drawing characters
# (prettier but each char is 3 bytes in UTF-8 — measurably more expensive
# on deep trees). "ascii" uses |- | `-.
style = "indent"

# Collapse single-child internal directories. Without this, narrow-deep
# paths like a/b/c/d/foo.rs render as four separate dir nodes.
collapse_single_child = true

# Sort children alphabetically. Off by default — source order is stable
# and predictable for LLMs.
sort = false
```

### Example

Before (`git status` raw porcelain):

```text
## main...origin/main
M  crates/tokf-cli/src/config/cache.rs
M  crates/tokf-cli/src/config/types.rs
M  crates/tokf-cli/src/main.rs
M  crates/tokf-cli/filters/git/diff.toml
M  crates/tokf-cli/filters/git/status.toml
?? crates/tokf-filter/src/filter/tree.rs
```

After (with `[tree]` enabled, indent style):

```text
main [synced]
crates/
  tokf-cli/
    src/
      config/
        M  cache.rs
        M  types.rs
      M  main.rs
    filters/git/
      M  diff.toml
      M  status.toml
  ?? tokf-filter/src/filter/tree.rs
```

The shared `crates/tokf-cli/` prefix is written once. The single-child chain `tokf-filter/src/filter/` collapses into one leaf. The model sees at a glance which directories cluster work.

### Pipeline position

The tree transform runs **after** `dedup` and **before** `on_success.output` / `max_lines`. Specifically: stage 2.6 in `apply_internal`, between dedup (2.5) and the lua/json/parse/section pipeline (2b–4).

### Constraints

- **Color restoration is bypassed** when `[tree]` is active. Tree-rendered lines are synthesized from path components, so per-line ANSI color spans from the original output don't survive structural rearrangement. If you need both colored output and tree structuring, you'll have to pick one.
- **Engagement is opt-in.** Without a `[tree]` section, filters behave exactly as before — no magic detection.
- **Engagement gates fail closed.** If `min_files` or `min_shared_depth` aren't met, the original flat lines pass through unchanged. There's no half-rendered intermediate state.
- **Rename arrows** like `R  old.rs -> new.rs` are handled: the path is split on ` -> ` and the suffix stays attached to the leaf. The trie key is the *old* path.
- **`[parse]` takes precedence.** A filter that declares both `[parse]` and `[tree]` will run parse and skip tree entirely. The two solve different problems (tree restructures path-list output, parse structures arbitrary text) and don't compose, so the precedence is fixed at parse-wins.

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

Detection has three modes, checked in order:

1. **File detection** (Phase A, before execution) — checks if config files exist in the current directory. First match wins.
2. **Args pattern** (Phase A.5, before execution) — regex-matches the remaining command-line arguments (joined with spaces). Fires after file detection but before the `passthrough_args` check, so a matched variant's own `passthrough_args` apply instead of the parent's.
3. **Output pattern** (Phase B, after execution) — regex-matches command output. Used as a fallback when no file or args pattern matched.

Args-pattern example — route `git diff --name-only` to a tree-structured child filter:

```toml
[[variant]]
name = "name-list"
detect.args_pattern = '--(name-only|name-status)'
filter = "git/diff-name-list"
```

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
