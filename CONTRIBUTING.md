# Contributing to tokf

tokf is an open source project built for the community. Contributions of all kinds are welcome — bug reports, filter additions, documentation improvements, and code changes.

---

## Getting started

```sh
git clone https://github.com/mpecan/tokf
cd tokf
cargo build
cargo test
just install-hooks   # install the pre-commit hook (run once after cloning)
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
cargo clippy --workspace --all-targets -- -D warnings
cargo test
```

All three must pass clean. The CI runs the same checks.

### Limits

- **Functions:** stay under 60 lines. Clippy enforces this.
- **Files:** aim for under 500 lines; CI warns at 500, fails at 700.
- **Test coverage:** minimum 80%, target 90%.

When a limit genuinely harms readability, it can be overridden with `#[allow(...)]` — but document the reason in a comment and get maintainer sign-off.

### Duplication

CI runs [cargo-dupes](https://crates.io/crates/cargo-dupes) to detect code duplication in production code (tests are excluded). Configuration lives in two files:

- **`dupes.toml`** — analysis settings and percentage thresholds (0.5% exact, 0.5% near)
- **`.dupes-ignore.toml`** — reviewed duplicates with documented reasons for each ignore

If `cargo dupes check` fails on your PR, either extract the shared logic or add an entry to `.dupes-ignore.toml` with a reason explaining why the duplication is acceptable.

```sh
cargo install cargo-dupes
cargo dupes              # full report
cargo dupes stats        # statistics only
cargo dupes check        # CI gate — fails if thresholds exceeded
```

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

For filters that need logic beyond what TOML can express, use the `[lua_script]` section with [Luau](https://luau.org/).

All Lua execution is sandboxed:

- **Blocked libraries:** `io`, `os`, `package` — no filesystem or network access.
- **Resource limits:** 1 million VM instructions, 16 MB memory (prevents infinite loops and memory exhaustion).

For local development, you can reference external scripts with `lua_script.file = "script.luau"`. For published filters, use inline `source` — `tokf publish` automatically inlines file references before uploading.

See `docs/lua-escape-hatch.md` for the full API, globals, and examples.

---

## Database & end-to-end tests

`tokf-server` uses CockroachDB. The DB integration tests and end-to-end tests are `#[ignore]`d by default — they only run when `DATABASE_URL` is set and you pass `--ignored`.

### Quick start with just

Copy `.env.example` to `.env` and adjust if needed (e.g. change `CONTAINER_RUNTIME` from `podman` to `docker`):

```sh
cp .env.example .env          # edit to choose podman or docker
just db-start                  # start CockroachDB
just db-status                 # verify it's running
just db-setup                  # create the test database
just test-db                   # run DB integration tests
just test-e2e                  # run end-to-end tests
just test-all                  # unit + DB + e2e tests
```

### Manual setup

Use Podman (or Docker) with the bundled compose file:

```sh
podman compose -f crates/tokf-server/docker-compose.yml up -d
```

This starts a single-node CockroachDB on port `26257` (SQL) and `8080` (admin UI).

```sh
export DATABASE_URL="postgresql://root@localhost:26257/tokf_test?sslmode=disable"
psql "postgresql://root@localhost:26257/defaultdb?sslmode=disable" \
  -c "CREATE DATABASE IF NOT EXISTS tokf_test"

# Unit tests (no database required)
cargo test --workspace

# DB integration tests (requires DATABASE_URL)
cargo test -p tokf-server -- --ignored

# End-to-end tests (requires DATABASE_URL)
cargo test -p e2e-tests -- --ignored
```

Each `#[crdb_test]` test creates its own isolated database, runs migrations, and cleans up afterwards. Tests can run in parallel without interfering with each other.

### Resetting the database

```sh
just db-reset                  # or manually:
podman compose -f crates/tokf-server/docker-compose.yml down -v
podman compose -f crates/tokf-server/docker-compose.yml up -d
```

---

## Pull requests

- Target the `main` branch
- Include tests for any changed behaviour
- Keep PRs focused — one feature or fix per PR
- Reference the relevant issue in the PR description (`Closes: #N`)

---

## License

By contributing you agree that your changes will be licensed under the [MIT License](LICENSE).
