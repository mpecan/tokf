---
name: tokf-filter
description: This skill should be used when the user asks to "create a filter", "write a tokf filter", "add a filter for <tool>", "how do I filter output", or needs guidance on tokf filter step types, templates, pipes, or placement conventions.
version: 0.1.0
---

# tokf Filter Authoring

You are an expert at writing tokf filter files. tokf is a config-driven CLI that compresses command output before it reaches an LLM context. Filters are TOML files that define how to process a command's output.

When the user asks you to create or modify a filter, follow this guide exactly. Produce valid, idiomatic TOML that matches the schema described below.

---

## Section 1 — What a Filter File Is

A filter file is a TOML file that describes:
- Which command(s) it applies to (`command`)
- How to transform the raw output (steps, applied in a fixed order)
- What to emit on success vs. failure

Filters live in three places, searched in priority order:

1. `.tokf/filters/` — project-local (repo-level overrides)
2. `~/.config/tokf/filters/` — user-level overrides
3. Built-in library (embedded in the tokf binary)

First match wins. Use `tokf which "cargo test"` to see which filter would activate for a given command.

---

## Section 2 — Processing Order

Steps execute in this fixed order — **do not rearrange them**:

1. **`match_output`** — whole-output substring checks; if matched, short-circuits the entire pipeline and emits immediately
2. **`[[replace]]`** — per-line regex transforms applied to every line, in array order
3. **`strip_ansi` / `trim_lines`** — per-line cleanup (ANSI stripping, whitespace trimming)
4. **`skip` / `keep`** — line-level filtering (drop or retain lines by regex)
5. **`dedup` / `dedup_window`** — collapse duplicate consecutive lines
6. **`lua_script`** — Luau escape hatch; runs after dedup, before section/parse
7. **`[[section]]` OR `[parse]`** — structured extraction (these are mutually exclusive; section is a state machine, parse is a declarative grouper)
8. **Exit-code branch** — `[on_success]` or `[on_failure]` depending on exit code
9. **`[fallback]`** — if neither `on_success` nor `on_failure` produced output
10. **`strip_empty_lines` / `collapse_empty_lines`** — post-processing cleanup on the final output

Within `[on_success]` and `[on_failure]`, fields are processed as:
- `head` / `tail` → trim lines
- `skip` / `extract` → further filter
- `aggregate` → reduce collected sections
- `output` → final template render

---

## Section 3 — Top-Level Fields Reference

| Field | Type | Default | Description |
|---|---|---|---|
| `command` | string or array of strings | required | Command pattern(s) to match. Supports `*` wildcard. |
| `run` | string | (same as command) | Override the actual command executed. Use `{args}` to forward arguments. |
| `match_output` | array of tables | `[]` | Whole-output checks. Short-circuit on first match. |
| `[[replace]]` | array of tables | `[]` | Per-line regex replacements, in order. |
| `skip` | array of strings (regex) | `[]` | Drop lines matching any regex. |
| `keep` | array of strings (regex) | `[]` | Retain only lines matching any regex. (Inverse of skip.) |
| `dedup` | bool | `false` | Collapse consecutive identical lines. |
| `dedup_window` | integer | `0` (off) | Dedup within a sliding window of N lines. |
| `strip_ansi` | bool | `false` | Strip ANSI escape sequences before skip/keep. |
| `trim_lines` | bool | `false` | Trim leading/trailing whitespace from each line. |
| `lua_script` | table | (absent) | Luau escape hatch. |
| `[[section]]` | array of tables | `[]` | State-machine section collectors. |
| `[parse]` | table | (absent) | Declarative structured parser (branch + group). |
| `[on_success]` | table | (absent) | Output branch for exit code 0. |
| `[on_failure]` | table | (absent) | Output branch for non-zero exit. |
| `[output]` | table | (absent) | Top-level output template (used by `[parse]`). |
| `[fallback]` | table | (absent) | Fallback when no branch matched. |
| `strip_empty_lines` | bool | `false` | Remove all blank lines from the final output. |
| `collapse_empty_lines` | bool | `false` | Collapse consecutive blank lines into one. |
| `[[variant]]` | array of tables | `[]` | Context-aware delegation to specialized child filters. |

---

## Section 4 — Step Types

### 4.1 `match_output` — Whole-Output Short-Circuit

Check the entire raw output for a substring. If matched, emit a fixed string and stop — no further processing.

```toml
match_output = [
  { contains = "Everything up-to-date", output = "ok (up-to-date)" },
  { contains = "rejected", output = "✗ push rejected (try pulling first)" },
]
```

- `contains`: literal substring to search for (case-sensitive)
- `output`: string to emit if matched
- `{line_containing}` template variable: the first line that contains the substring

```toml
match_output = [
  { contains = "error", output = "Error on: {line_containing}" },
]
```

**When to use**: for well-known one-liner outcomes that make the rest of filtering irrelevant (e.g., "already up to date", "nothing to push", "authentication failed").

---

### 4.2 `[[replace]]` — Per-Line Regex Transforms

Applied to every line, in array order, before skip/keep. Use to reformat noisy lines.

```toml
[[replace]]
pattern = '^(\S+)\s+\S+\s+(\S+)\s+(\S+)'
output = "{1}: {2} → {3}"

[[replace]]
pattern = '^\s+Compiling (\S+) v(\S+)'
output = "compiling {1}@{2}"
```

- `pattern`: Rust regex (RE2 syntax, no lookaheads)
- `output`: template with `{1}`, `{2}`, … for capture groups; `{0}` is the full match
- If the pattern doesn't match a line, that line passes through unchanged
- Invalid patterns are silently skipped at runtime

**When to use**: when a line contains useful information but in a verbose format — reformat it rather than dropping it.

---

### 4.3 `skip` / `keep` — Line Filtering

`skip` drops lines matching any regex. `keep` retains only lines matching any regex. They compose:

```toml
skip = [
  "^\\s*Compiling ",
  "^\\s*Downloading ",
  "^\\s*$",
]

keep = ["^error", "^warning"]
```

- Both are arrays of regex strings
- Applied after `[[replace]]`
- `skip` is checked first, then `keep`
- A line must pass both: not skipped, and (if keep is non-empty) matching keep

**When to use**: `skip` for removing known noise patterns; `keep` for allow-listing (e.g., keep only lines that start with `error` or `warning`).

Also available inside `[on_success]` and `[on_failure]` for branch-level filtering.

---

### 4.4 `dedup` / `dedup_window` — Deduplication

```toml
dedup = true           # collapse consecutive identical lines
dedup_window = 10      # dedup within a 10-line sliding window
```

- `dedup = true`: removes consecutive duplicate lines (like `uniq`)
- `dedup_window = N`: deduplicates within a sliding window of N lines (catches near-consecutive repeats)
- They are independent; you can use both

**When to use**: for commands that emit repetitive progress lines (e.g., `npm install` printing the same package multiple times, spinner frames, repeated warnings).

---

### 4.5 `lua_script` — Luau Escape Hatch

For logic that pure TOML cannot express: numeric math, multi-line lookahead, conditional branching.

```toml
[lua_script]
lang = "luau"
source = '''
if exit_code == 0 then
    return "passed"
else
    local msg = output:match("Error: (.+)") or "unknown error"
    return "FAILED: " .. msg
end
'''
```

**Globals available**:
- `output` (string): the full output after skip/keep/dedup
- `exit_code` (integer): the command's exit code
- `args` (table of strings): the arguments passed to the command

**Return semantics**:
- Return a string → replaces output, skips remaining TOML pipeline
- Return `nil` → fall through to `[[section]]` / `[parse]` / `[on_success]` / `[on_failure]`

**Sandbox**: `io`, `os`, and `package` are blocked. No filesystem or network access. Standard math/string/table libraries are available.

**When to use**: only when no TOML step can express the logic. Most filters do not need this. Consider it after exhausting `match_output`, `skip/keep`, `[[replace]]`, `[[section]]`, and `[parse]`.

---

### 4.6 `[[section]]` — State-Machine Section Collector

The most powerful step. Defines a state machine that collects lines into named variables as it scans top-to-bottom.

```toml
[[section]]
name = "failures"
enter = "^failures:$"      # regex: start collecting when this matches
exit = "^failures:$"       # regex: stop collecting when this matches (after start)
split_on = "^\\s*$"        # regex: split collected lines into blocks on blank lines
collect_as = "failure_blocks"

[[section]]
name = "summary"
match = "^test result:"    # regex: collect only lines matching this (no enter/exit)
collect_as = "summary_lines"
```

**Fields**:
| Field | Required | Description |
|---|---|---|
| `name` | yes | Identifier for this section (used in error messages) |
| `enter` | no | Regex to start collecting (state transitions to "inside") |
| `exit` | no | Regex to stop collecting (state transitions to "outside") |
| `match` | no | Collect any line matching this regex, without enter/exit state |
| `split_on` | no | Split collected lines into blocks when this regex matches |
| `collect_as` | yes | Variable name to bind the result to |

**Accessing collected variables in templates**:
| Expression | Type | Description |
|---|---|---|
| `{name}` | string | Full collected text joined with newlines |
| `{name.lines}` | collection | Individual lines as a list |
| `{name.blocks}` | collection | Blocks split by `split_on` |
| `{name.count}` | integer | Number of blocks (or lines if no split_on) |

**When to use**: when the output has distinct sections with clear start/end markers — test failure blocks, error sections, file change groups.

---

### 4.7 `[parse]` — Declarative Structured Parser

Alternative to `[[section]]` for commands with table-like output. Declaratively extracts a header field and groups remaining lines.

```toml
[parse]
branch = { line = 1, pattern = '## (\S+?)(?:\.\.\.(\S+))?(?:\s+\[(.+)\])?$', output = "{1}" }

[parse.group]
key = { pattern = '^(.{2}) ', output = "{1}" }
labels = { "M " = "modified", "??" = "untracked", "D " = "deleted" }

[output]
format = """
{branch}{tracking_info}
{group_counts}"""
group_counts_format = "  {label}: {count}"
empty = "clean — nothing to commit"
```

**`[parse]` fields**:
| Field | Description |
|---|---|
| `branch` | Extract a single value from a specific line (`line`, `pattern`, `output`) |
| `[parse.group]` | Group remaining lines by a key pattern |

**`[parse.group]` fields**:
| Field | Description |
|---|---|
| `key` | `{ pattern, output }` — extract the grouping key from each line |
| `labels` | Map from raw key string to human-readable label |

**`[output]` fields** (used with `[parse]`):
| Field | Description |
|---|---|
| `format` | Template string for the overall output |
| `group_counts_format` | Template for each group entry: `{label}`, `{count}` |
| `empty` | String to emit when no lines were grouped |

**When to use**: for commands like `git status`, `docker ps`, `kubectl get` — table-formatted output where you want to extract a header and count/group rows.

---

### 4.8 `[on_success]` / `[on_failure]` — Exit Code Branches

These branches run after all top-level steps. They have their own sub-fields:

```toml
[on_success]
output = "ok ✓ {2}"          # template; collected variables are available
head = 20                     # keep first N lines
tail = 10                     # keep last N lines
skip = ["^\\s*$"]            # additional line filtering
extract = { pattern = '(\S+)\s*->\s*(\S+)', output = "ok ✓ {2}" }
aggregate = { from = "summary_lines", pattern = 'ok\. (\d+) passed', sum = "passed", count_as = "suites" }

[on_failure]
tail = 10
output = "FAILED: {summary_lines | join: \"\\n\"}"
```

**Branch sub-fields**:
| Field | Description |
|---|---|
| `output` | Template string for the output. Has access to all collected `[[section]]` variables. `{output}` = the filtered output text. |
| `head` | Keep first N lines of filtered output |
| `tail` | Keep last N lines of filtered output |
| `skip` | Array of regexes to filter output lines within this branch |
| `extract` | `{ pattern, output }` — find first match, render template with capture groups |
| `aggregate` | Reduce collected section lines into numeric summaries |

**`aggregate` fields**:
| Field | Description |
|---|---|
| `from` | Variable name (a `collect_as` result from `[[section]]`) |
| `pattern` | Regex with one capture group to extract a number |
| `sum` | Variable name to bind the sum to |
| `count_as` | Variable name to bind the count (number of lines matched) to |

**When to use**: Always. Every filter should have at least one of `[on_success]` or `[on_failure]`. Use `[on_success]` to produce a clean summary. Use `[on_failure]` to show enough context to diagnose the issue.

---

### 4.9 `[fallback]` — Last Resort

Emits output when neither `[on_success]` nor `[on_failure]` produced anything.

```toml
[fallback]
tail = 5
```

**When to use**: as a safety net when you have complex branching logic. Ensures tokf never silently swallows output.

---

### 4.10 `[[variant]]` — Context-Aware Filter Delegation

Some commands are wrappers around different underlying tools (e.g. `npm test` may run Jest, Vitest, or Mocha). A parent filter can declare `[[variant]]` entries that delegate to specialized child filters based on project context.

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

**Fields**:

| Field | Type | Required | Description |
|---|---|---|---|
| `name` | string | yes | Human-readable identifier for this variant |
| `detect.files` | array of strings | no | File paths to check in CWD (pre-execution detection) |
| `detect.output_pattern` | string (regex) | no | Regex to match against command output (post-execution fallback) |
| `filter` | string | yes | Filter to delegate to (relative path without `.toml`, e.g. `"npm/test-vitest"`) |

**Two-phase detection**:
1. **File detection** (before execution) — checks if any listed config files exist in the current directory. First match wins.
2. **Output pattern** (after execution) — regex-matches the command output. Used as a fallback when no file was detected.

At least one of `detect.files` or `detect.output_pattern` must be set.

**Behavior**:
- When a variant matches, the child filter **replaces** the parent entirely — no field inheritance or merging
- When no variant matches, the parent filter's own fields (`skip`, `on_success`, etc.) apply as the fallback
- The `filter` field references another filter by its discovery name (e.g. `"npm/test-vitest"` maps to `filters/npm/test-vitest.toml`)

**TOML ordering**: `[[variant]]` entries must appear **after** all top-level fields (`skip`, `[on_success]`, etc.) because TOML array-of-tables sections capture subsequent keys.

**When to use**: when a single command pattern maps to different underlying tools that produce fundamentally different output formats. Create a parent filter with a generic fallback, then create specialized child filters for each tool.

---

## Section 5 — Template Pipes

Output templates support pipe chains: `{var | pipe | pipe: "arg"}`.

| Pipe | Input → Output | Description |
|---|---|---|
| `lines` | Str → Collection | Split string on newlines into a list |
| `join: "sep"` | Collection → Str | Join list items with separator string |
| `each: "tmpl"` | Collection → Collection | Map each item through a sub-template; `{value}` = item, `{index}` = 1-based index |
| `keep: "re"` | Collection → Collection | Retain items matching the regex |
| `where: "re"` | Collection → Collection | Alias for `keep:` |
| `truncate: N` | Str → Str | Truncate to N characters, appending `…` |

**Examples**:

Filter a multi-line output variable to only error lines:
```toml
[on_failure]
output = "{output | lines | keep: \"^error\" | join: \"\\n\"}"
```

For each collected block, show only `>` (pointer) and `E` (assertion) lines:
```toml
[on_failure]
output = "{failure_blocks | each: \"{value | lines | keep: \\\"^[>E] \\\"}\" | join: \"\\n\"}"
```

Truncate long lines and number them:
```toml
[on_failure]
output = "{summary_lines | each: \"{index}. {value | truncate: 120}\" | join: \"\\n\"}"
```

---

## Section 6 — Naming & Placement Conventions

**File naming**:
- `filters/<tool>/<subcommand>.toml` for two-word commands: `filters/git/push.toml` for `git push`
- `filters/<tool>.toml` for single-word commands: `filters/pytest.toml` for `pytest`
- For wildcards: `filters/npm/run.toml` with `command = "npm run *"` in the TOML
- Lowercase filenames only, no spaces

**Placement**:
| Location | Purpose |
|---|---|
| `.tokf/filters/` | Project-local override (committed to the repo) |
| `~/.config/tokf/filters/` | User-level override (your personal filters) |
| `filters/` in the tokf source repo | Built-in library (requires a tokf release) |

When creating a filter for a user's project, default to `.tokf/filters/` unless they specify otherwise.

**Command field**:
- Exact match: `command = "git push"` matches `git push` and `git push origin main`
- Wildcard: `command = "npm run *"` matches `npm run dev`, `npm run build`, etc.
- Array: `command = ["cargo test", "cargo t"]` matches either form

---

## Section 7 — Workflow for Creating a New Filter

Follow these steps when asked to create a filter:

### Step 1: Understand the command's output

Ask the user to provide (or capture) example output from the command. If they don't have it, generate a plausible example based on the tool's known output format. Look for:
- What's signal (errors, results, summaries)
- What's noise (progress bars, compilation lines, download progress, blank lines)
- What patterns mark sections (e.g., "failures:", "test result:")

### Step 2: Choose the right complexity level

| Level | When to use | Steps to use |
|---|---|---|
| Level 1 (simple) | Command produces one-liner outcomes | `match_output`, `skip`, `extract` |
| Level 2 (structured) | Table-like output needing grouping | `[parse]` + `[output]` |
| Level 3 (stateful) | Multi-section output with nested structure | `[[section]]` + `aggregate` + pipes |

Start at the lowest level that handles the use case. Don't reach for `[[section]]` when `skip` + `extract` suffices.

### Step 3: Draft the filter

1. Set `command` to match the command pattern
2. Add `match_output` for well-known short-circuit cases (empty output, auth failure, "already done")
3. Add `skip` to drop noise lines (progress, compile output, blank lines)
4. Add `[[replace]]` to reformat noisy-but-useful lines
5. Add `[[section]]` or `[parse]` if you need structured extraction
6. Write `[on_success]` with the desired output format
7. Write `[on_failure]` with enough context to diagnose (`tail = 20` is a safe default)
8. Add `[fallback]` with `tail = 5` as a safety net for complex filters

### Step 4: Write test cases and verify

Create a `<stem>_test/` directory adjacent to the filter TOML and add at least one test case per meaningful outcome (success, failure, edge cases):

```
filters/mytool/
  mysubcmd.toml       ← filter config
  mysubcmd_test/      ← test suite
    success.toml
    failure.toml
```

Each test case is a TOML file:

```toml
name = "success shows one clean line"
fixture = "tests/fixtures/mytool_success.txt"  # path relative to this file, then CWD
exit_code = 0

[[expect]]
equals = "ok ✓"

[[expect]]
not_contains = "noise"
```

Or with an inline fixture (no file needed):

```toml
name = "known error message"
inline = "Error: connection refused\n"
exit_code = 1

[[expect]]
contains = "connection refused"
```

Run the suite:

```sh
tokf verify mytool/mysubcmd   # run one suite
tokf verify                   # run all suites
```

For quick one-off testing without creating test files:

```sh
tokf test filters/mytool/mysubcmd.toml tests/fixtures/mytool_output.txt --exit-code 0
```

### Step 5: Place and name the file correctly

- Two-word command: `.tokf/filters/mytool/mysubcmd.toml`
- Single-word command: `.tokf/filters/mytool.toml`
- Wildcard command: `.tokf/filters/mytool/run.toml` with `command = "mytool run *"`

---

## Section 8 — Three Annotated Examples

### Example 1: `git push` (Level 1 — match_output + extract)

Goal: 15 lines of push noise → "ok ✓ main" (or failure message).

```toml
# filters/git/push.toml — Level 1
# Raw output: 15 lines of object counting, compression, "remote:" lines
# Filtered (success): "ok ✓ main"
# Filtered (up-to-date): "ok (up-to-date)"
# Filtered (rejected): "✗ push rejected (try pulling first)"

command = "git push"

# Check full output for well-known outcomes before any processing
match_output = [
  { contains = "Everything up-to-date", output = "ok (up-to-date)" },
  { contains = "rejected", output = "✗ push rejected (try pulling first)" },
]

[on_success]
# Drop all the noise lines
skip = [
  "^Enumerating objects:",
  "^Counting objects:",
  "^Delta compression",
  "^Compressing objects:",
  "^Writing objects:",
  "^Total \\d+",
  "^remote:",
  "^To ",
]
# Extract the branch name from the ref update line: "abc1234..def5678  main -> main"
extract = { pattern = '(\S+)\s*->\s*(\S+)', output = "ok ✓ {2}" }

[on_failure]
tail = 10
```

**Key decisions**:
- `match_output` handles the two most common "instant" outcomes
- `extract` captures the branch name from the ref update line
- `tail = 10` on failure gives enough context without overwhelming

---

### Example 2: `git status` (Level 2 — parse + group)

Goal: 30+ lines of verbose status → branch name + grouped file counts.

```toml
# filters/git/status.toml — Level 2
# Raw output: 30+ lines with hints, file paths, status codes
# Filtered: "main [ahead 2]\n  modified: 3\n  untracked: 2"

command = "git status"

# Override: use porcelain format for reliable machine parsing
run = "git status --porcelain -b"

match_output = [
  { contains = "not a git repository", output = "Not a git repository" },
]

[parse]
# First line: "## main...origin/main [ahead 2]"
# Extract: branch name, upstream, ahead/behind info
branch = { line = 1, pattern = '## (\S+?)(?:\.\.\.(\S+))?(?:\s+\[(.+)\])?$', output = "{1}" }

[parse.group]
# Group remaining lines by their two-character status code
key = { pattern = '^(.{2}) ', output = "{1}" }
labels = {
  "M " = "modified",
  " M" = "modified (unstaged)",
  "MM" = "modified (staged+unstaged)",
  "A " = "added",
  "??" = "untracked",
  "D " = "deleted",
  " D" = "deleted (unstaged)",
  "R " = "renamed",
  "UU" = "conflict",
  "AM" = "added+modified"
}

[output]
format = """
{branch}{tracking_info}
{group_counts}"""
group_counts_format = "  {label}: {count}"
empty = "clean — nothing to commit"
```

**Key decisions**:
- `run` overrides to porcelain format — machine-readable is easier to parse
- `[parse]` extracts the branch header line declaratively
- `[parse.group]` groups by status code without needing `[[section]]`
- `[output]` uses built-in `{group_counts}` variable populated by the parser

---

### Example 3: `cargo test` (Level 3 — section + aggregate + pipes)

Goal: 200+ lines with compile noise, per-test "ok" lines, failure blocks → one-liner on pass, structured failure report on fail.

```toml
# filters/cargo/test.toml — Level 3
# Raw output: 200+ lines
# Filtered (pass): "✓ cargo test: 137 passed (24 suites)"
# Filtered (fail): failure details + summary

command = "cargo test"

# Drop all the noise
skip = [
  "^\\s*Compiling ",
  "^\\s*Downloading ",
  "^\\s*Downloaded ",
  "^\\s*Finished ",
  "^\\s*Locking ",
  "^running \\d+ tests?$",
  "^test .+ \\.\\.\\. ok$",   # individual passing tests
  "^\\s*$",
  "^\\s*Doc-tests ",
]

# State machine: collect the "failures:" section into blocks split by blank lines
[[section]]
name = "failures"
enter = "^failures:$"
exit = "^failures:$"
split_on = "^\\s*$"
collect_as = "failure_blocks"

# Collect just the failure names (short list before the blocks)
[[section]]
name = "failure_names"
enter = "^failures:$"
exit = "^\\s*$"
match = "^\\s+\\S+"
collect_as = "failure_list"

# Collect "test result: ok/FAILED" summary lines (one per test suite)
[[section]]
name = "summary"
match = "^test result:"
collect_as = "summary_lines"

# Success: aggregate "N passed" across all suite summaries
[on_success]
aggregate = { from = "summary_lines", pattern = 'ok\. (\d+) passed', sum = "passed", count_as = "suites" }
output = "✓ cargo test: {passed} passed ({suites} suites)"

# Failure: show numbered, truncated failure blocks + full summary
[on_failure]
output = """
FAILURES ({failure_blocks.count}):
═══════════════════════════════════════
{failure_blocks | each: "{index}. {value | truncate: 200}" | join: "\\n"}

{summary_lines | join: "\\n"}"""

# Safety net: if sections didn't collect (very short output), show last 5 lines
[fallback]
tail = 5
```

**Key decisions**:
- `skip` removes all per-test "ok" lines — only failures and summaries remain
- Three `[[section]]` collectors handle different structural parts of the output
- `aggregate` sums "N passed" across multiple suite summary lines
- Pipe chain `{failure_blocks | each: "..." | join: "\\n"}` formats numbered, truncated blocks
- `[fallback]` catches edge cases where cargo emits very short output (e.g., no tests found)

---

## Section 9 — Writing Test Cases

Every filter in the standard library has a `<stem>_test/` directory with declarative test cases. When writing or modifying a filter, write test cases alongside it.

### Test case format

```toml
name = "success output is a single clean line"   # required, human-readable
fixture = "tests/fixtures/cargo_build_success.txt"  # path to raw output file
# inline = "some raw output\nline two"            # alternative: inline fixture
exit_code = 0                                    # optional, default 0
args = []                                        # optional, forwarded to filter

[[expect]]
equals = "ok ✓"          # exact match

[[expect]]
not_contains = "Compiling"  # noise must be gone
```

### Assertion types

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

Every `[[expect]]` entry checks one assertion. A test case with multiple `[[expect]]` entries must pass all of them. A test case with no `[[expect]]` entries is an error.

### What to test

For every filter, write at least:
- **Success case**: the happy path produces the expected one-liner or summary
- **Failure case**: a failing exit code produces enough context to diagnose
- **Edge cases**: cover each `match_output` branch (e.g., "up-to-date", "rejected")

### Directory convention

```
filters/
  git/
    push.toml          ← filter config
    push_test/         ← test suite (identified by _test suffix)
      success.toml
      up_to_date.toml
      rejected.toml
      failure.toml
```

The `_test` suffix makes suite directories immediately identifiable in file listings and distinguishes them from filter category directories.

---

## Section 10 — Common Mistakes to Avoid

1. **Don't use `keep` when `skip` is enough.** `keep` is an allow-list — it drops everything that doesn't match. Use it only when you want to radically filter to a specific type of line.

2. **Escape backslashes in TOML strings.** In regular strings, `\\d` means literal `\d` in the regex. In TOML raw strings (`'...'`), backslashes are literal. Use raw strings for complex patterns.

3. **`match_output` is a short-circuit.** If it matches, nothing else runs. Don't put it at the end expecting it to be a fallback — it runs first.

4. **`[[section]]` and `[parse]` are mutually exclusive.** Use one or the other, not both.

5. **`{output}` in branch templates is the filtered output text** (after skip/keep/replace/dedup), not the raw command output.

6. **Pipe chains need careful quoting.** When nesting templates inside `each:`, escape inner quotes: `{each: "{value | lines | keep: \\\"^error\\\"}"}`.

7. **Don't skip the `[fallback]`.** Complex filters with `[[section]]` can produce empty output if sections don't match. Always add `[fallback] tail = 5` as a safety net.

8. **Test with realistic fixture data.** A filter that works on a trimmed example may miss edge cases. Use real command output saved to a `.txt` fixture file.
