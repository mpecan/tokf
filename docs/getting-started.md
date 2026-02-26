---
title: Getting Started
description: Install tokf and run your first filtered command in minutes.
order: 1
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
