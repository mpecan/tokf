# tokf

**[tokf.net](https://tokf.net)** — a config-driven CLI that compresses command output before it reaches an LLM context.

Commands like `git push`, `cargo test`, or `docker build` produce verbose output full of progress bars, compile noise, and boilerplate. tokf intercepts that output, applies a TOML filter, and emits only what matters. Less context consumed, cleaner signal for the model.

---

## How it works

```
tokf run git push origin main
```

tokf looks up a filter for `git push`, runs the command, and applies the filter. A full push output of 15 lines becomes one:

```
ok ✓ main
```

The filter logic lives in plain TOML files — no recompilation required. Anyone can author, share, or override a filter.

---

## Installation

```sh
cargo install tokf
```

Or build from source:

```sh
git clone https://github.com/mpecan/tokf
cd tokf
cargo build --release
# binary at target/release/tokf
```

### Claude Code hook

tokf integrates with Claude Code as a `PreToolUse` hook that automatically filters `Bash` tool output:

```sh
tokf hook install          # project-local (.tokf/)
tokf hook install --global # user-level (~/.config/tokf/)
```

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

### Explore available filters

```sh
tokf ls                    # list all filters
tokf which "cargo test"    # which filter would match
tokf show git/push         # print the TOML source
```

### Flags

| Flag | Description |
|---|---|
| `--timing` | Print how long filtering took |
| `--verbose` | Show which filter was matched |
| `--no-filter` | Pass output through without filtering |
| `--no-cache` | Bypass the filter discovery cache |

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
| `cargo/install` | `cargo install` |
| `cargo/test` | `cargo test` |
| `docker/*` | `docker build`, `docker ps`, … |
| `npm/*` | `npm install`, `npm run`, … |
| `pnpm/*` | pnpm equivalents |
| `go/*` | `go build`, `go test`, … |
| `gh/*` | GitHub CLI commands |
| `kubectl/*` | Kubernetes CLI |
| `next/*` | Next.js dev/build |
| `pytest` | Python test runner |
| `tsc` | TypeScript compiler |

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

match_output = [              # whole-output substring checks, short-circuit the pipeline
  { contains = "rejected", output = "push rejected" },
]

[on_success]                  # branch for exit code 0
output = "ok ✓ {2}"          # template; {output} = pre-filtered output

[on_failure]                  # branch for non-zero exit
tail = 10                     # keep the last N lines
```

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

## Acknowledgements

tokf was heavily inspired by [rtk](https://github.com/rtk-ai/rtk) ([rtk-ai.app](https://www.rtk-ai.app/)) — a CLI proxy that compresses command output before it reaches an AI agent's context window. rtk pioneered the idea and demonstrated that 60–90% context reduction is achievable across common dev tools. tokf takes a different approach (TOML-driven filters, user-overridable library, Claude Code hook integration) but the core insight is theirs.

---

## License

MIT — see [LICENSE](LICENSE).
