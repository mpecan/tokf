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

Check the [issue tracker](https://github.com/mpecan/tokf/issues) for open issues. Good first contributions include adding new filters, improving existing ones, or expanding documentation.

If you want to add a new built-in filter, no issue is required — just open a PR with the TOML file, a `_test/` suite, and fixture data.

---

## Commits

Use [Conventional Commits](https://www.conventionalcommits.org/):

```
<type>(<scope>): <description>
```

Types: `feat`, `fix`, `refactor`, `test`, `docs`, `chore`, `ci`, `perf`, `build`

Scopes: `config`, `filter`, `runner`, `output`, `cli`, `hook`, `tracking`, `history`

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
4. Create a `<subcommand>_test/` directory adjacent to the TOML with declarative test cases
5. Save real command output as fixture `.txt` files (inline fixtures work for short outputs)
6. Run `tokf verify <tool>/<subcommand>` to validate

Example test case (`filters/git/push_test/success.toml`):

```toml
name = "successful push shows branch"
fixture = "success.txt"
exit_code = 0

[[expect]]
starts_with = "ok"
```

Use `tokf test filters/my/filter.toml tests/fixtures/my_fixture.txt` to iterate quickly on a single fixture.

Every filter in the stdlib **must** have a `_test/` suite — CI enforces this with `tokf verify --require-all`.

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
