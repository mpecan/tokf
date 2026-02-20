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

Scopes: `config`, `filter`, `runner`, `output`, `cli`, `hook`, `tracking`

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
- **Declarative filter tests**: place test cases in a `<stem>_test/` directory adjacent to the filter TOML (e.g. `filters/git/push_test/` next to `filters/git/push.toml`). Each case is a TOML file with `name`, `inline` or `fixture`, `exit_code`, and `[[expect]]` blocks. Run with `tokf verify`.
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
  config/
    mod.rs         — Config loading, file resolution
    types.rs       — Serde structs for the TOML schema
  filter/
    mod.rs         — FilterEngine orchestration
    skip.rs        — Skip/keep line filtering
    extract.rs     — Regex capture and template interpolation
    group.rs       — Line grouping by key pattern
    section.rs     — State machine section parsing
    aggregate.rs   — Sum/count across collected items
  runner.rs        — Command execution, stdout/stderr capture
  output.rs        — Template rendering, variable interpolation
filters/           — Standard library of filter configs (.toml)
  git/
    push.toml      — filter config
    push_test/     — declarative test suite for git/push
tests/
  integration/     — End-to-end tests
  fixtures/        — Sample command outputs for testing
```

### Design Decisions (Do Not Revisit)
- **TOML** for config. Not YAML, not JSON.
- **Capture then process**, not streaming.
- **First match wins** for config resolution. No merging, no inheritance.
- **Passthrough on missing filter.** Never block a command because a filter doesn't exist.
- **Exit code propagation.** tokf must return the same exit code as the underlying command.

## Build & Run

```sh
cargo build              # Build
cargo test               # Run all tests
cargo clippy -- -D warnings  # Lint
cargo fmt -- --check     # Format check
```

## What Not To Do

- Don't add features beyond what the current issue asks for.
- Don't add dependencies for Phase 3/4 concerns during Phase 1.
- Don't implement streaming, hot-reloading, HTTP registries, GUIs, parallel execution, output caching, or advanced linting. These are explicitly deferred.
- Don't sacrifice user experience for implementation convenience.
