# tokf — Development Guidelines

## Project Philosophy

tokf is an open source project built for the community. We are not looking for profits — this exists for open source sake. Every decision should prioritize:

- **End-user experience** — whether the user is a human or an LLM, the tool should be intuitive, fast, and transparent about what it's doing.
- **Visibility** — users should always understand what tokf is doing. Stderr notes, `--timing`, `--verbose` flags. Never hide behavior.
- **Transparency** — clear error messages, honest documentation, no dark patterns.

## Commits

Use **Conventional Commits** strictly:

```
<type>(<scope>): <description>

[optional body]

[optional footer]
```

Types: `feat`, `fix`, `refactor`, `test`, `docs`, `chore`, `ci`, `perf`, `build`

Scopes: `config`, `filter`, `runner`, `output`, `cli`, `hook`, `tracking`, `history`

Examples:
- `feat(filter): implement skip/keep line filtering`
- `fix(config): handle missing optional fields in git-status.toml`
- `test(filter): add fixtures for cargo-test failure case`
- `ci: add clippy and fmt checks to GitHub Actions`

Keep commits atomic — one logical change per commit. Don't bundle unrelated changes.

## Code Quality

### Testing
- **Minimum 80% coverage, target 90%.**
- Every module gets unit tests. Every filter gets integration tests with fixture data.
- Fixture-driven: save real command outputs as `.txt` files in `tests/fixtures/`. Tests load fixtures, apply filters, assert on output. No dependency on external tools in tests.
- **Declarative filter tests**: place test cases in a `<stem>_test/` directory adjacent to the filter TOML (e.g. `filters/git/push_test/` next to `filters/git/push.toml`). Each case is a TOML file with `name`, `inline` or `fixture`, `exit_code`, and `[[expect]]` blocks. Run with `tokf verify`. Every filter in the stdlib **must** have a `_test/` suite — CI enforces this with `tokf verify --require-all`.
- Run `cargo test` after every meaningful change. Tests must pass before committing.

### Pragmatism

We are pragmatic. The limits below are guidelines that produce better code in the vast majority of cases. When a limit actively harms readability or forces an awkward split, it can be exceeded — but this requires explicit approval from the maintainer. Document the reason in a code comment when overriding.

### Linting & Formatting
- `cargo fmt` before every commit. No exceptions.
- `cargo clippy -- -D warnings` must pass clean.
- Functions should stay under 60 lines (enforced via `clippy.toml`). Can be overridden with `#[allow()]` when approved.
- Source files:
  - **Soft limit: 500 lines** — aim to split before this. CI warns.
  - **Hard limit: 700 lines** — CI fails. Requires approval to override.

### Duplication
- Keep duplication low. If you see the same logic in two places, extract it — but only when it's genuinely the same concern, not just superficially similar.
- DRY applies to logic, not to test setup. Test clarity beats test brevity.

### Dependencies
- Use reputable, well-maintained crates instead of reinventing. Check download counts, maintenance activity, and dependency footprint before adding.
- Keep the dependency tree tight. Don't add a crate for something the standard library handles.
- Pin versions in `Cargo.toml`. Review what transitive dependencies you're pulling in.

## Architecture

### File Structure
```
src/
  main.rs          — CLI entry, argument parsing, subcommand routing
  lib.rs           — Public module declarations
  resolve.rs       — Filter resolution, command execution, tracking (binary crate)
  runner.rs        — Command execution, stdout/stderr capture
  baseline.rs      — Fair baseline computation for piped commands
  config/
    mod.rs         — Config loading, file discovery, pattern matching
    types.rs       — Serde structs for the TOML schema
    variant.rs     — Two-phase variant resolution (file detection + output pattern)
    cache.rs       — Binary config cache (rkyv serialization)
  filter/
    mod.rs         — FilterEngine orchestration
    skip.rs        — Skip/keep line filtering
    extract.rs     — Regex capture and template interpolation
    replace.rs     — Per-line regex replacement
    group.rs       — Line grouping by key pattern
    section.rs     — State machine section parsing
    aggregate.rs   — Sum/count across collected items
    template.rs    — Template rendering, variable interpolation, pipe chains
    match_output.rs — Whole-output substring matching
    dedup.rs       — Line deduplication
    parse.rs       — Declarative structured parser (branch + group)
    cleanup.rs     — ANSI stripping, line trimming, blank line handling
    lua.rs         — Luau script escape hatch
  rewrite/         — Shell rewrite engine (hook + CLI)
  hook/            — Claude Code PreToolUse hook handler + installer
  tracking/        — Token savings tracking (SQLite)
  history/         — Filtered output history (SQLite)
  skill.rs         — Claude Code skill installer
  verify_cmd.rs    — Declarative test suite runner
  eject_cmd.rs     — Filter ejection to local/global config
  cache_cmd.rs     — Cache management subcommand
  gain.rs          — Token savings display
  history_cmd.rs   — History subcommand
filters/           — Standard library of filter configs (.toml)
  git/             — git add, commit, diff, log, push, show, status
  cargo/           — cargo build, check, clippy, install, test
  npm/             — npm run, npm/pnpm/yarn test (with vitest/jest variants)
  docker/          — docker build, compose, images, ps
  go/              — go build, go vet
  gradle/          — gradle build, test, dependencies
  gh/              — GitHub CLI (pr, issue)
  kubectl/         — kubectl get pods
  next/            — next build
  pnpm/            — pnpm add, install
  prisma/          — prisma generate
tests/
  cli_*.rs         — End-to-end CLI integration tests
  filter_*.rs      — Filter pipeline integration tests
  fixtures/        — Sample command outputs for testing
```

### Design Decisions (Do Not Revisit)
- **TOML** for config. Not YAML, not JSON.
- **Capture then process**, not streaming.
- **First match wins** for config resolution. No merging, no inheritance.
- **Passthrough on missing filter.** Never block a command because a filter doesn't exist.
- **Exit code propagation.** tokf must return the same exit code as the underlying command.
- **Variant delegation, not inheritance.** Parent filters delegate to child filters via `[[variant]]` — the child filter replaces the parent entirely, it doesn't inherit or merge fields.
- **Two-phase variant detection.** File detection (pre-execution) takes priority; output-pattern matching (post-execution) is the fallback. Parent config applies when no variant matches.

## Build & Run

```sh
cargo build              # Build
cargo test               # Run all tests
cargo clippy -- -D warnings  # Lint
cargo fmt -- --check     # Format check
```

## What Not To Do

- Don't add features beyond what the current issue asks for.
- Don't implement streaming, hot-reloading, HTTP registries, GUIs, parallel execution, output caching, or advanced linting. These are explicitly deferred.
- Don't sacrifice user experience for implementation convenience.
