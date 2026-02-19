# Contributing to tokf

tokf is an open source project built for the community. Contributions of all kinds are welcome — bug reports, filter additions, documentation improvements, and code changes.

---

## Getting started

```sh
git clone https://github.com/mpecan/tokf
cd tokf
cargo build
cargo test
```

The project requires a recent stable Rust toolchain. See `rust-toolchain.toml` for the pinned version.

---

## What to work on

Check the [issue tracker](https://github.com/mpecan/tokf/issues) for open issues. Issues are labelled by phase:

- **Phase 1** (core runtime) — filter engine, CLI, config resolution
- **Phase 2** (hook rewrite) — Claude Code hook integration
- **Phase 3** (token tracking) — SQLite-backed savings stats
- **Phase 4** (Lua escape hatch) — scripted filter logic

If you want to add a new built-in filter, no issue is required — just open a PR with the TOML file and, if the output format is non-trivial, a fixture file.

---

## Commits

Use [Conventional Commits](https://www.conventionalcommits.org/):

```
<type>(<scope>): <description>
```

Types: `feat`, `fix`, `refactor`, `test`, `docs`, `chore`, `ci`, `perf`, `build`

Scopes: `config`, `filter`, `runner`, `output`, `cli`, `hook`, `tracking`

Keep commits atomic — one logical change per commit.

---

## Code quality

Before opening a PR:

```sh
cargo fmt
cargo clippy -- -D warnings
cargo test
```

All three must pass clean. The CI runs the same checks.

### Limits

- **Functions:** stay under 60 lines. Clippy enforces this.
- **Files:** aim for under 500 lines; CI warns at 500, fails at 700.
- **Test coverage:** minimum 80%, target 90%.

When a limit genuinely harms readability, it can be overridden with `#[allow(...)]` — but document the reason in a comment and get maintainer sign-off.

---

## Adding a built-in filter

1. Create `filters/<tool>/<subcommand>.toml`
2. Set `command` to the pattern users type (e.g. `"git push"`)
3. Add `[on_success]` and/or `[on_failure]` branches
4. Save a real command output as `tests/fixtures/<tool>_<subcommand>_<case>.txt`
5. Add integration tests in `tests/filter_<tool>_<subcommand>.rs`

Run `tokf test filters/my/filter.toml tests/fixtures/my_fixture.txt` to iterate quickly without a full `cargo test`.

---

## Lua filters

For filters that need logic beyond what TOML can express, use the `[lua_script]` section with [Luau](https://luau.org/). The sandbox blocks `io`, `os`, and `package` — scripts cannot access the filesystem or network.

See the [README](README.md#lua-escape-hatch) for the full API and the built-in filter library for examples.

---

## Pull requests

- Target the `main` branch
- Include tests for any changed behaviour
- Keep PRs focused — one feature or fix per PR
- Reference the relevant issue in the PR description (`Closes: #N`)

---

## License

By contributing you agree that your changes will be licensed under the [MIT License](LICENSE).
