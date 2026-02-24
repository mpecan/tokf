# tokf Filter Step Reference

Exhaustive field-level documentation for every step type. This is the authoritative reference — treat it like a man page.

---

## `command`

**Type**: `string` or `array of strings`
**Required**: yes

The command pattern this filter matches against. tokf compares the beginning of the user's command against this value.

```toml
command = "git push"           # matches: git push, git push origin main
command = "npm run *"          # wildcard: matches npm run dev, npm run build, etc.
command = ["cargo test", "cargo t"]  # array: matches either form
```

**Wildcard rules**:
- `*` matches any sequence of characters (including spaces)
- Only one `*` per command string is supported
- The wildcard always appears at the end: `"npm run *"`, not `"* run npm"`

**Array matching**: each entry in the array is checked independently. First match in the array wins.

---

## `run`

**Type**: `string`
**Required**: no
**Default**: the matched command is executed as-is

Override the actual command executed. Useful when you want to match on one command string but run a different (often machine-readable) variant.

```toml
command = "git status"
run = "git status --porcelain -b"
```

**Template variables in `run`**:
- `{args}` — the arguments the user passed after the matched command prefix
- `{line_containing}` — not available here (only in `match_output.output`)

```toml
command = "cargo test"
run = "cargo test --no-fail-fast {args}"
```

---

## `match_output`

**Type**: `array of tables`
**Required**: no
**Default**: `[]`

Whole-output substring checks. Evaluated **before any other processing**. If a check matches, its `output` is emitted and the pipeline stops entirely.

```toml
match_output = [
  { contains = "Everything up-to-date", output = "ok (up-to-date)" },
  { contains = "rejected", output = "✗ push rejected" },
  { contains = "error:", output = "Error: {line_containing}" },
]
```

**Per-entry fields**:

| Field | Type | Required | Description |
|---|---|---|---|
| `contains` | string | yes | Literal substring to search for in the full output (case-sensitive) |
| `output` | string | yes | String to emit if `contains` is found in the output |

**Template variables available in `output`**:
- `{line_containing}` — the first line in the output that contains the `contains` substring

**Behavior**:
- Checks are evaluated in array order; first match wins
- Comparison is against the raw, unprocessed output (before skip/keep/replace)
- If no entry matches, the pipeline continues normally

---

## `[[replace]]`

**Type**: array of tables (TOML array of inline tables)
**Required**: no
**Default**: `[]`

Per-line regex transforms. Applied to every output line, in array order, before `skip`/`keep`. Each `[[replace]]` block defines one transformation.

```toml
[[replace]]
pattern = '^(\S+)\s+\S+\s+(\S+)\s+(\S+)'
output = "{1}: {2} → {3}"

[[replace]]
pattern = '^\s+Compiling (\S+) v(\S+)'
output = "compiling {1}@{2}"
```

**Fields**:

| Field | Type | Required | Description |
|---|---|---|---|
| `pattern` | string | yes | Rust regex pattern (RE2 syntax). Must match the full line anchor is not required — partial matches are allowed. |
| `output` | string | yes | Output template. `{0}` = full match. `{1}`, `{2}`, … = capture groups. |

**Behavior**:
- If `pattern` does not match a line, that line passes through unchanged
- If `pattern` matches, the line is replaced with the rendered `output` template
- Multiple `[[replace]]` blocks are applied in sequence — output of one feeds into the next
- Invalid regex patterns are silently ignored at runtime (line passes through)
- Transforms are applied before `skip`/`keep` — this means a replaced line can then be filtered

**Regex notes**:
- Use Rust regex syntax (similar to RE2, no lookaheads/lookbehinds)
- In TOML regular strings: `\\d` = `\d`, `\\s` = `\s`, etc.
- In TOML raw strings (`'...'`): `\d` = `\d` (no extra escaping needed)

---

## `skip`

**Type**: `array of strings` (each is a regex)
**Required**: no
**Default**: `[]`
**Available at**: top-level, `[on_success]`, `[on_failure]`

Drop lines matching any of the given regexes.

```toml
skip = [
  "^\\s*Compiling ",
  "^\\s*Downloading ",
  "^\\s*$",
  "^running \\d+ tests?$",
]
```

**Behavior**:
- A line is dropped if it matches **any** regex in the array
- Regexes are partial-match by default (no implicit `^` or `$`)
- Applied after `[[replace]]` at top level
- When used inside `[on_success]` / `[on_failure]`, applied after `head`/`tail`

---

## `keep`

**Type**: `array of strings` (each is a regex)
**Required**: no
**Default**: `[]` (keep all)
**Available at**: top-level, `[on_success]`, `[on_failure]`

Retain only lines matching any of the given regexes. Inverse of `skip`.

```toml
keep = ["^error", "^warning", "^FAILED"]
```

**Behavior**:
- A line is retained only if it matches **at least one** regex in the array
- When both `skip` and `keep` are set: a line must not match any `skip` **and** must match at least one `keep`
- An empty `keep` array means "keep all" (no filtering)

---

## `dedup`

**Type**: `bool`
**Required**: no
**Default**: `false`

Collapse consecutive identical lines (like Unix `uniq`).

```toml
dedup = true
```

**Behavior**:
- Only collapses lines that are consecutive and exactly identical
- Does not affect non-consecutive duplicates (use `dedup_window` for that)

---

## `dedup_window`

**Type**: `integer`
**Required**: no
**Default**: `0` (disabled)

Deduplicate within a sliding window of N lines. More aggressive than `dedup = true`.

```toml
dedup_window = 10
```

**Behavior**:
- A line is dropped if it appears anywhere in the preceding N lines
- Setting `dedup_window = 1` is equivalent to `dedup = true`
- `dedup` and `dedup_window` are independent; you can use both

---

## `[lua_script]`

**Type**: table
**Required**: no

Luau escape hatch for logic that TOML cannot express.

```toml
[lua_script]
lang = "luau"
source = '''
if exit_code == 0 then
    return "passed ✓"
else
    local count = 0
    for _ in output:gmatch("FAILED") do count = count + 1 end
    return count .. " test(s) FAILED"
end
'''
```

**Fields**:

| Field | Type | Required | Description |
|---|---|---|---|
| `lang` | string | yes | Must be `"luau"` |
| `source` | string | one of `source`/`file` | Inline Luau script source code |
| `file` | string | one of `source`/`file` | Path to a `.luau` file (resolved relative to CWD) |

Exactly one of `source` or `file` must be set. Use `file` when the script is long or shared across filters.

**Globals available inside the script**:

| Global | Type | Description |
|---|---|---|
| `output` | string | Full output text after skip/keep/dedup/replace |
| `exit_code` | integer | Command exit code (0 = success) |
| `args` | table | Arguments passed to the command (1-indexed table of strings) |

**Return semantics**:
- Return a string → replaces output entirely; `[on_success]`/`[on_failure]`/`[[section]]` are skipped
- Return `nil` (or don't return) → fall through to the rest of the pipeline
- Runtime errors propagate as tokf errors

**Sandbox**:
- `io`, `os`, `package` are blocked (no filesystem, network, or process access)
- Standard Luau libraries available: `math`, `string`, `table`, `utf8`
- `require` is blocked

---

## `[[section]]`

**Type**: array of tables (TOML array of inline tables)
**Required**: no
**Default**: `[]`
**Incompatible with**: `[parse]`

State-machine collector that routes lines into named variables.

```toml
[[section]]
name = "failures"
enter = "^failures:$"
exit = "^test result:"
split_on = "^\\s*$"
collect_as = "failure_blocks"

[[section]]
name = "summary"
match = "^test result:"
collect_as = "summary_lines"
```

**Fields**:

| Field | Type | Required | Description |
|---|---|---|---|
| `name` | string | yes | Identifier for this section (used in error messages and debugging) |
| `enter` | string (regex) | no | Start collecting when a line matches this regex. Transitions state to "inside". |
| `exit` | string (regex) | no | Stop collecting when a line matches this regex (checked after entering). Transitions state to "outside". |
| `match` | string (regex) | no | Collect any line matching this regex, regardless of state. Cannot be combined with `enter`/`exit`. |
| `split_on` | string (regex) | no | When inside, lines matching this regex act as block separators (split collected lines into blocks). |
| `collect_as` | string | yes | Variable name to bind collected content to. |

**State machine rules**:
- Sections are evaluated top-to-bottom for each line
- Multiple sections can match the same line independently
- `enter` and `match` are mutually exclusive — pick one pattern style per section
- The `exit` line itself is **not** included in the collected content
- The `enter` line itself **is** included in the collected content

**Accessing collected variables**:

| Expression | Type | Description |
|---|---|---|
| `{name}` | string | Full collected text, lines joined with `\n` |
| `{name.lines}` | collection | Individual collected lines as a list |
| `{name.blocks}` | collection | Blocks split by `split_on` as a list of strings |
| `{name.count}` | integer | Number of blocks (if `split_on` set) or number of lines |

---

## `[parse]`

**Type**: table
**Required**: no
**Incompatible with**: `[[section]]`

Declarative structured parser for table-like command output. Designed for commands like `git status --porcelain`, `docker ps`, `kubectl get pods`.

```toml
[parse]
branch = { line = 1, pattern = '## (\S+?)(?:\.\.\.(\S+))?', output = "{1}" }

[parse.group]
key = { pattern = '^(.{2}) ', output = "{1}" }
labels = { "M " = "modified", "??" = "untracked", "D " = "deleted" }
```

**`[parse]` fields**:

| Field | Type | Required | Description |
|---|---|---|---|
| `branch` | inline table | no | Extract a single value from a specific line number |

**`branch` fields**:

| Field | Type | Required | Description |
|---|---|---|---|
| `line` | integer | yes | 1-based line number to extract from |
| `pattern` | string (regex) | yes | Pattern to match against that line |
| `output` | string | yes | Template for the extracted value; `{1}`, `{2}`, … for capture groups |

**`[parse.group]` fields**:

| Field | Type | Required | Description |
|---|---|---|---|
| `key` | inline table | yes | How to extract the grouping key from each line |
| `labels` | inline table (string → string) | no | Map from raw key string to human-readable label |

**`key` fields**:

| Field | Type | Required | Description |
|---|---|---|---|
| `pattern` | string (regex) | yes | Pattern to match against a line to extract the group key |
| `output` | string | yes | Template for the key value; `{1}`, `{2}`, … for capture groups |

---

## `[output]`

**Type**: table
**Required**: no (but usually paired with `[parse]`)

Defines the output format when using `[parse]`.

```toml
[output]
format = """
{branch}{tracking_info}
{group_counts}"""
group_counts_format = "  {label}: {count}"
empty = "clean — nothing to commit"
```

**Fields**:

| Field | Type | Required | Description |
|---|---|---|---|
| `format` | string | yes | Overall output template. Has access to `{branch}` (from `parse.branch`), `{tracking_info}`, `{group_counts}`. |
| `group_counts_format` | string | no | Template for each group entry. Variables: `{label}` (from `labels` map), `{count}` (number of lines in group). Default: `{label}: {count}` |
| `empty` | string | no | String to emit when no lines were grouped (empty output). Default: empty string. |

---

## `[on_success]`

**Type**: table
**Required**: no

Output branch executed when the command exits with code 0.

```toml
[on_success]
output = "ok ✓ {branch}"
head = 20
tail = 10
skip = ["^\\s*$"]
extract = { pattern = '(\S+)\s*->\s*(\S+)', output = "ok ✓ {2}" }
aggregate = { from = "summary_lines", pattern = 'ok\. (\d+) passed', sum = "passed", count_as = "suites" }
```

**Fields**:

| Field | Type | Description |
|---|---|---|
| `output` | string | Template for the final output. Has access to all `[[section]]` variables and `{output}` (filtered output text). |
| `head` | integer | Keep only the first N lines of filtered output. |
| `tail` | integer | Keep only the last N lines of filtered output. |
| `skip` | array of strings | Additional regexes to filter output lines within this branch. |
| `extract` | inline table | Find the first matching line, render a template with capture groups. |
| `aggregate` | inline table | Reduce section lines into numeric summaries. |

**`extract` fields**:

| Field | Type | Required | Description |
|---|---|---|---|
| `pattern` | string (regex) | yes | Pattern to search for (first match wins) |
| `output` | string | yes | Template with `{1}`, `{2}`, … for capture groups |

**`aggregate` fields**:

| Field | Type | Required | Description |
|---|---|---|---|
| `from` | string | yes | Variable name (must match a `collect_as` value from `[[section]]`) |
| `pattern` | string (regex) | yes | Regex with one integer capture group to extract a number from each line |
| `sum` | string | no | Variable name to bind the sum of all extracted numbers to |
| `count_as` | string | no | Variable name to bind the count of matched lines to |

---

## `[on_failure]`

**Type**: table
**Required**: no

Output branch executed when the command exits with a non-zero code. Has the same fields as `[on_success]`.

```toml
[on_failure]
tail = 20
output = """
FAILED ({failure_blocks.count} failures):
{failure_blocks | each: "{index}. {value | truncate: 200}" | join: "\\n"}
{summary_lines | join: "\\n"}"""
```

Same fields as `[on_success]`. See above.

**Recommendation**: Always include `[on_failure]` with at least `tail = 20`. Users debugging failures need context.

---

## `[fallback]`

**Type**: table
**Required**: no

Emitted when neither `[on_success]` nor `[on_failure]` produced output.

```toml
[fallback]
tail = 5
```

**Fields**:

| Field | Type | Description |
|---|---|---|
| `tail` | integer | Keep the last N lines of filtered output |

**When this triggers**: if `[on_success]` or `[on_failure]` has an `output` template that renders to empty, or if neither branch is defined for the given exit code, `[fallback]` activates.

**Recommendation**: Always add `[fallback] tail = 5` to complex filters using `[[section]]`. Acts as a safety net for edge cases.

---

## `strip_ansi`

**Type**: `bool`
**Required**: no
**Default**: `false`

Strip ANSI escape sequences (colors, bold, cursor movement) from each line before skip/keep pattern matching. Applied after `[[replace]]`, before `skip`/`keep`.

```toml
strip_ansi = true
```

**When to use**: for commands that emit colored output (e.g. test runners, linters). Without this, ANSI codes can interfere with regex matching in `skip`, `keep`, and `[[replace]]`.

---

## `trim_lines`

**Type**: `bool`
**Required**: no
**Default**: `false`

Trim leading and trailing whitespace from each line. Applied alongside `strip_ansi`, before `skip`/`keep`.

```toml
trim_lines = true
```

**When to use**: when command output has inconsistent indentation that would complicate regex patterns.

---

## `strip_empty_lines`

**Type**: `bool`
**Required**: no
**Default**: `false`

Remove all blank lines from the final output. Applied as a post-processing step after all other filtering.

```toml
strip_empty_lines = true
```

**When to use**: when the filtered output has excessive blank lines that waste tokens without adding structure.

---

## `collapse_empty_lines`

**Type**: `bool`
**Required**: no
**Default**: `false`

Collapse consecutive blank lines into a single blank line in the final output. Less aggressive than `strip_empty_lines`. Applied as a post-processing step after all other filtering.

```toml
collapse_empty_lines = true
```

**When to use**: when you want to preserve some visual structure (single blank lines as separators) but remove redundant consecutive blanks.

---

## `[[variant]]`

**Type**: array of tables
**Required**: no
**Default**: `[]`

Context-aware delegation to specialized child filters. Used when a single command pattern (e.g. `npm test`) can invoke different underlying tools (Jest, Vitest, Mocha) that produce fundamentally different output.

```toml
command = ["npm test", "pnpm test", "yarn test"]

strip_ansi = true
skip = ["^> "]

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

[[variant]]
name = "mocha"
detect.output_pattern = "passing|failing|pending"
filter = "npm/test-mocha"
```

**Per-entry fields**:

| Field | Type | Required | Description |
|---|---|---|---|
| `name` | string | yes | Human-readable identifier for this variant (used in diagnostics and `--verbose` output) |
| `detect.files` | array of strings | no | File paths to check for existence in the current working directory |
| `detect.output_pattern` | string (regex) | no | Regex to match against the full command output |
| `filter` | string | yes | Filter to delegate to, specified as a relative path without `.toml` (e.g. `"npm/test-vitest"`) |

At least one of `detect.files` or `detect.output_pattern` must be set per variant.

**Two-phase detection**:

1. **File detection (Phase A, pre-execution)**: For each variant with `detect.files`, tokf checks if any of the listed files exist in the current working directory. First file match wins — that variant's filter is used immediately.
2. **Output pattern (Phase B, post-execution)**: Variants with only `detect.output_pattern` are deferred. After the command runs, tokf regex-matches the output against each deferred variant's pattern. First match wins.

File detection takes priority because it happens before execution and avoids unnecessary output analysis.

**Delegation semantics**:
- When a variant matches, the child filter **replaces** the parent entirely — no field inheritance or merging
- When no variant matches, the parent filter's own fields apply as the fallback
- The child filter is a standalone TOML file with its own `command`, `skip`, `on_success`, etc.

**TOML ordering**: `[[variant]]` entries must appear **after** all top-level fields and tables (`skip`, `[on_success]`, `[on_failure]`, etc.) because TOML array-of-tables sections capture all subsequent keys into the array entries.

**Debugging**: Use `tokf which "npm test" -v` to see variant resolution details.

---

## `show_history_hint`

**Type**: `bool`
**Required**: no
**Default**: `false`

When enabled, tokf appends a hint line after the filtered output telling the reader how to retrieve the full, unfiltered output from history.

```toml
command = "git status"
show_history_hint = true

[on_success]
output = "{branch} — {counts}"
```

**Output example**:

```
main — modified: 3, untracked: 1
Filtered - full output: `tokf history show --raw 42`
```

**Behavior**:
- The hint line is appended to stdout so both humans and LLMs can see it
- The history entry stores the clean filtered output without the hint line
- tokf also auto-appends the hint when it detects the same command running twice in a row (repetition detection), regardless of this setting

**When to use**: for filters that aggressively compress output (e.g. `git status` reducing 30 lines to 1 line), where the LLM consumer might need access to the full output for detailed analysis.
