# tokf — Token Filter Framework

## Implementation Spec for Claude Code

### What We're Building

A config-driven CLI tool that compresses command output before it reaches an LLM context.
Unlike rtk (which hardcodes a filter per command in Rust), this tool loads filter definitions
from TOML files. Users, teams, and models can author new filters without recompilation.

The binary is small. The intelligence lives in the filter configs.

### Name

`tokf` (token filter). Short, available, descriptive. Change it if you find a conflict.

---

## Phase 1: Core Runtime

**Goal:** `tokf run "git push"` loads `git-push.toml`, executes the command, applies the
filter, prints compressed output. This is the vertical slice that proves the architecture.

### Project Setup

```
cargo init tokf
```

Dependencies (keep this list tight):

- `clap` (derive) — CLI parsing
- `toml` + `serde` — config loading
- `regex` — pattern matching
- `anyhow` — error handling

Do NOT add yet: Lua, SQLite, colored output, walking/globbing. Those come later.

### Directory Structure

```
tokf/
├── Cargo.toml
├── src/
│   ├── main.rs          # CLI entry, argument parsing, subcommand routing
│   ├── config/
│   │   ├── mod.rs        # Config loading, file resolution
│   │   └── types.rs      # Serde structs for the TOML schema
│   ├── filter/
│   │   ├── mod.rs         # FilterEngine: takes raw output + config → filtered output
│   │   ├── skip.rs        # Skip/keep line filtering
│   │   ├── extract.rs     # Regex capture → template interpolation
│   │   ├── group.rs       # Line grouping by key pattern
│   │   ├── section.rs     # State machine section parsing
│   │   └── aggregate.rs   # Sum/count across collected items
│   ├── runner.rs          # Command execution, stdout/stderr capture
│   └── output.rs          # Template rendering, variable interpolation
├── filters/               # Standard library of filter configs
│   ├── git-push.toml
│   ├── git-status.toml
│   ├── git-log.toml
│   ├── git-diff.toml
│   ├── git-add.toml
│   ├── git-commit.toml
│   ├── cargo-test.toml
│   └── ls.toml
└── tests/
    ├── integration/       # End-to-end: raw input → filtered output
    └── fixtures/          # Sample command outputs for testing
```

### Config Schema (TOML types)

Implement these serde structs in `config/types.rs`. This is the contract — get it right.

```rust
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Deserialize)]
pub struct FilterConfig {
    /// The command pattern this filter matches (e.g., "git push", "cargo test")
    pub command: String,

    /// Optional: override the actual command to execute
    /// e.g., "git status --porcelain -b" instead of "git status"
    pub run: Option<String>,

    /// Lines to always skip (regex patterns)
    #[serde(default)]
    pub skip: Vec<String>,

    /// Lines to always keep (if set, ONLY these lines pass through)
    #[serde(default)]
    pub keep: Vec<String>,

    /// Multi-step commands (run multiple commands sequentially)
    #[serde(default)]
    pub step: Vec<Step>,

    /// Extraction rules: capture groups → output template
    pub extract: Option<ExtractRule>,

    /// Whole-output pattern matching (checked before line-by-line processing)
    #[serde(default)]
    pub match_output: Vec<MatchOutputRule>,

    /// Section-based parsing (state machine)
    #[serde(default)]
    pub section: Vec<Section>,

    /// Success-path formatting
    pub on_success: Option<OutputBranch>,

    /// Failure-path formatting
    pub on_failure: Option<OutputBranch>,

    /// Parse block for grouping and structured extraction
    pub parse: Option<ParseConfig>,

    /// Fallback: if nothing else matched, what to do
    pub fallback: Option<FallbackConfig>,
}

#[derive(Debug, Deserialize)]
pub struct Step {
    /// Command to run. {args} interpolates the original arguments.
    pub run: String,
    /// Name to reference this step's output in the output template
    #[serde(rename = "as")]
    pub as_name: String,
    /// Optional pipeline of transforms to apply
    #[serde(default)]
    pub pipeline: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct ExtractRule {
    /// Regex with capture groups
    pub pattern: String,
    /// Output template using {1}, {2}, etc. for capture groups
    pub output: String,
}

#[derive(Debug, Deserialize)]
pub struct MatchOutputRule {
    /// String to search for in the output
    pub contains: String,
    /// What to output instead
    pub output: String,
}

#[derive(Debug, Deserialize)]
pub struct Section {
    /// Name for referencing collected lines
    pub name: String,
    /// Regex: enter this section when a line matches
    pub enter: Option<String>,
    /// Regex: exit this section when a line matches
    pub exit: Option<String>,
    /// Regex: match individual lines (non-stateful, for simple collection)
    #[serde(rename = "match")]
    pub match_pattern: Option<String>,
    /// Split collected lines into blocks on this pattern
    pub split_on: Option<String>,
    /// Name for the collected data
    pub collect_as: String,
}

#[derive(Debug, Deserialize)]
pub struct OutputBranch {
    /// Simple output template with variable interpolation
    pub output: Option<String>,
    /// Aggregation rule
    pub aggregate: Option<AggregateRule>,
    /// Show last N lines
    pub tail: Option<usize>,
    /// Show first N lines
    pub head: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct AggregateRule {
    /// Which collected data to aggregate
    pub from: String,
    /// Regex to extract numbers
    pub pattern: String,
    /// Capture group to sum
    pub sum: Option<String>,
    /// Name for the count of matched items
    pub count_as: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ParseConfig {
    /// Extract from a specific line
    pub branch: Option<LineExtract>,
    /// Group lines by pattern
    pub group: Option<GroupConfig>,
}

#[derive(Debug, Deserialize)]
pub struct LineExtract {
    /// 1-based line number
    pub line: usize,
    /// Regex with capture groups
    pub pattern: String,
    /// Output template
    pub output: String,
}

#[derive(Debug, Deserialize)]
pub struct GroupConfig {
    /// Regex to extract the grouping key
    pub key: ExtractRule,
    /// Map raw keys to human labels
    #[serde(default)]
    pub labels: HashMap<String, String>,
}

#[derive(Debug, Deserialize)]
pub struct FallbackConfig {
    /// Show last N lines if nothing else matched
    pub tail: Option<usize>,
}
```

When implementing: start with the struct definitions, then write unit tests that deserialize
real TOML filter files into these structs. Get the deserialization working and tested before
writing any filter logic.

### Config Resolution

Implement in `config/mod.rs`. Search order:

```
./.tokf/filters/{name}.toml          # repo-specific
~/.config/tokf/filters/{name}.toml   # user-level
{binary_dir}/filters/{name}.toml     # shipped stdlib
```

First match wins. The lookup key is derived from the command: `git push` → `git-push.toml`,
`cargo test` → `cargo-test.toml`. The mapping is: take the command words, join with `-`.

If no filter file is found: execute the command unfiltered and print output as-is (passthrough).
Never fail silently. Print a one-line note to stderr: `[tokf] no filter for "terraform plan", passing through`.

### Command Execution (`runner.rs`)

Simple. No streaming for now.

```rust
pub struct CommandResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub combined: String, // stdout + "\n" + stderr
}

pub fn execute(command: &str, args: &[String]) -> Result<CommandResult>;
```

Use `std::process::Command`. Capture both stdout and stderr. Combine them into `combined`
(most filters work on combined output). Propagate the exit code to the caller.

For commands specified as strings (from `run =` in config), execute via `sh -c`.

### Filter Engine (`filter/mod.rs`)

This is the core. It takes `CommandResult` + `FilterConfig` and returns the filtered string.

Processing order:

1. Check `match_output` rules against the full output. If any match, return immediately.
2. Apply `skip` patterns — remove matching lines.
3. Apply `keep` patterns — if present, only keep matching lines (skip and keep are mutually exclusive; if both set, keep wins).
4. Run `section` state machine — route lines into named collections.
5. Apply `extract` rule — if set, scan remaining lines for the pattern.
6. Apply `parse.group` — if set, bucket remaining lines.
7. Apply `aggregate` rules from `on_success` / `on_failure`.
8. Render `output` template with all collected variables.

Implement each step as its own function. Test each independently with fixture data.

The template renderer (`output.rs`) handles:
- `{variable_name}` — simple substitution
- `{collection | each: "template" | join: "sep"}` — iterate and join
- `{collection.count}` — count of items in a collection
- `{value | truncate: N}` — truncate to N chars

Start with simple `{variable}` substitution only. Add the pipe syntax when you need it
for the `cargo-test.toml` filter.

### CLI Interface

```
tokf run <command> [args...]     # Run command through filter
tokf check <filter.toml>        # Validate a filter file (parse + report errors)
tokf test <filter.toml> <fixture_file>  # Dry-run: apply filter to saved output
tokf ls                          # List all available filters and their source
```

`tokf run` is the primary command. The others are for authoring/debugging filters.

`tokf test` is critical for the model-authoring workflow: a model writes a filter,
tests it against sample output without executing anything, iterates until correct.

### Performance Targets

These are the constraints. Test them. Don't optimize prematurely but don't ignore them.

| Metric | Target | Why |
|--------|--------|-----|
| Config load + parse | < 5ms | Adds to every command invocation |
| Filter processing (simple) | < 2ms for < 1KB output | git push, git add — must be instant |
| Filter processing (complex) | < 20ms for < 100KB output | cargo test with 200 lines |
| Total overhead vs raw command | < 30ms | User should not notice |
| Binary size | < 10MB | Downloadable, installable quickly |
| Memory | < 50MB RSS | Don't buffer excessively |

Measure from the start. Add a `--timing` flag that prints `[tokf] filter took 1.2ms` to stderr.
This is cheap to implement and invaluable for development.

### Phase 1 Deliverables

Build and test in this order:

1. **Config types + deserialization tests.** Write the structs, write 3 test TOML files
   (git-push, git-status, cargo-test), verify they parse correctly.

2. **Command runner.** Execute a command, capture output. Test with `echo hello`.

3. **Simple filter: skip + extract + match_output.** Implement enough to make `git-push.toml`
   work. Write an integration test with fixture output (a saved `git push` output).

4. **Grouping filter.** Implement `parse.group` for `git-status.toml`. Test with
   fixture of `git status --porcelain -b` output.

5. **Section state machine.** Implement `section` for `cargo-test.toml`. This is the
   hardest piece. Test with fixture of cargo test output (passing case, failing case).

6. **Template rendering.** Implement variable substitution + pipe syntax.

7. **Config resolution.** File discovery across the three directories.

8. **CLI integration.** Wire it all together with clap.

9. **Write the initial filter stdlib.** At minimum: git-push, git-add, git-commit,
   git-status, git-log, cargo-test, ls. These prove the DSL covers real use cases.

Each step should have tests before moving to the next. Use fixture files (saved command
outputs) as test inputs — don't depend on git/cargo being available in tests.

---

## Phase 2: Rewrite Hook

**Goal:** `tokf` can intercept commands in Claude Code via a PreToolUse hook, using a
declarative rewrite table instead of a bash script.

### Rewrite Config

Add a `rewrites.toml` that maps commands to their tokf equivalents:

```toml
# ~/.config/tokf/rewrites.toml

# Skip patterns: never rewrite these
[skip]
patterns = ["^tokf ", "<<"]

# Rewrite rules: first match wins
[[rewrite]]
match = "^git (status|diff|log|add|commit|push|pull|branch|fetch|stash|show)(\\s|$)"
to = "tokf run git {1} {rest}"

[[rewrite]]
match = "^cargo (test|build|clippy|check|install|nextest|fmt)(\\s|$)"
to = "tokf run cargo {1} {rest}"

[[rewrite]]
match = "^cat (.+)"
to = "tokf run cat {1}"

[[rewrite]]
match = "^ls(\\s|$)"
to = "tokf run ls {rest}"
```

### Hook Script

Generate a minimal shell hook that:
1. Reads stdin (the Claude Code PreToolUse JSON)
2. Extracts the command
3. Calls `tokf rewrite <command>` which outputs the rewritten command (or the original)
4. Emits the JSON response

```
tokf hook install --global    # Generate hook + update ~/.claude/settings.json
tokf hook install             # Generate hook for current project
tokf rewrite "git status"    # Returns "tokf run git status" or empty if no match
```

The hook shell script should be as thin as possible — just a shim that calls `tokf rewrite`.
All matching logic lives in the compiled binary reading `rewrites.toml`.

### Phase 2 Deliverables

1. Rewrite config parsing (same pattern as filter config).
2. `tokf rewrite` subcommand — stdin or argument, outputs rewritten command.
3. Hook script generator.
4. Integration test: simulate PreToolUse JSON → verify correct rewrite.

---

## Phase 3: Token Tracking

**Goal:** `tokf gain` shows how many tokens filters have saved.

### Storage

SQLite database at `~/.local/share/tokf/tracking.db`.

Schema:

```sql
CREATE TABLE events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp TEXT NOT NULL,
    command TEXT NOT NULL,
    filter_name TEXT,
    input_bytes INTEGER NOT NULL,
    output_bytes INTEGER NOT NULL,
    input_tokens_est INTEGER NOT NULL,  -- bytes / 4 approximation
    output_tokens_est INTEGER NOT NULL,
    filter_time_ms INTEGER NOT NULL,
    exit_code INTEGER NOT NULL
);
```

Add `rusqlite` (bundled) dependency in this phase.

### Tracking Integration

After every filtered command, insert a row. This should be non-blocking — if the DB write
fails, log to stderr and continue. Never let tracking break the primary flow.

### CLI

```
tokf gain                    # Summary: total commands, tokens saved, percentage
tokf gain --daily            # Daily breakdown
tokf gain --by-filter        # Breakdown by filter name
tokf gain --json             # Machine-readable export
```

### Phase 3 Deliverables

1. SQLite setup + migration.
2. Tracking insertion after each `tokf run`.
3. `tokf gain` subcommand with summary stats.
4. `--daily`, `--by-filter`, `--json` flags.

---

## Phase 4: Script Escape Hatch

**Goal:** Filters can embed Lua for cases the declarative DSL can't handle.

### Dependency

Add `mlua` with the `luau` feature (Luau is Lua 5.1 compatible, faster, sandboxed by
default). This is the only addition to the dependency tree.

### Integration

In a filter config:

```toml
[pipeline]
script = "lua"
source = """
local lines = {}
for line in output:gmatch("[^\n]+") do
  if not line:match("^%s*Compiling") then
    table.insert(lines, line)
  end
end
return table.concat(lines, "\n")
"""
```

Or referencing an external file:

```toml
[pipeline]
script = "lua"
file = "custom-filter.lua"
```

### Lua API Surface

Keep it minimal. The Lua script receives:

- `output` (string) — the command's combined output
- `stdout` (string) — just stdout
- `stderr` (string) — just stderr
- `exit_code` (number) — the exit code
- `args` (table) — the original command arguments

It must return a string (the filtered output).

No filesystem access. No network access. No `os.execute`. The sandbox is the feature.

### Phase 4 Deliverables

1. `mlua` integration with Luau.
2. Script execution in the filter pipeline.
3. Sandboxing verification (no io, no os).
4. One complex filter rewritten using Lua to prove it works.

---

## Phase 5: Community / Sharing

**Goal:** Users can install filter packs from a registry.

This is deliberately last. The local experience must be solid before adding distribution.

```
tokf install gh:username/tokf-filters    # Install from GitHub repo
tokf install terraform                    # Install from a future registry
tokf list --installed                     # Show installed filter packs
```

Defer the design of this phase until Phases 1–4 are complete and battle-tested.

---

## Implementation Principles

### For Claude Code Specifically

- **Run tests after every meaningful change.** `cargo test` after each module.
- **Fixture-driven testing.** Save real command outputs as `.txt` files in `tests/fixtures/`.
  Tests load fixtures, apply filters, assert on output. No dependency on external tools.
- **One module at a time.** Don't scaffold everything upfront. Build config types → test them
  → build runner → test it → build skip filter → test it → build next filter op → test it.
- **Commit at each phase boundary.** Working, tested code at each phase.

### Design Decisions (Do Not Revisit)

- **TOML, not YAML.** TOML is unambiguous, has native datetime support, and the Rust
  ecosystem support is excellent.
- **Capture then process, not streaming.** Simplifies the filter engine enormously.
  The performance targets allow this for any reasonable command output.
- **First match wins for config resolution.** Repo config overrides user overrides stdlib.
  No merging. No inheritance.
- **Passthrough on missing filter.** Never block a command because a filter doesn't exist.
  The tool is additive.
- **Exit code propagation.** tokf must return the same exit code as the underlying command.
  LLM agents rely on exit codes for control flow.

### What to Defer

Do NOT implement these in any phase. They are future concerns:

- Streaming output processing
- Filter config hot-reloading
- HTTP-based filter registry
- GUI / TUI for filter authoring
- Parallel command execution in multi-step filters
- Output caching
- Filter config linting beyond `tokf check`
