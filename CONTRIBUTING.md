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

## Workspace structure

The repository is a Cargo workspace with six crates:

| Crate | Type | Description |
|---|---|---|
| `crates/tokf-cli` | bin + lib | CLI binary (`tokf`) — filter resolution, command execution, tracking, hooks |
| `crates/tokf-common` | lib | Shared types and utilities (config hash, serde helpers) |
| `crates/tokf-filter` | lib | Pure filter engine — all TOML-driven processing steps and Lua sandbox |
| `crates/tokf-server` | bin + lib | Remote server — auth, publishing, sync, gain API (axum + CockroachDB) |
| `crates/crdb-test-macro` | proc-macro | `#[crdb_test]` attribute macro for CockroachDB integration tests |
| `crates/e2e-tests` | test-only | End-to-end tests spanning CLI, server, and database |

Build or test individual crates with `-p`:

```sh
cargo build -p tokf-cli            # build the CLI only
cargo test -p tokf-filter          # test the filter engine
cargo clippy -p tokf-server -- -D warnings  # lint the server
```

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

### Supply chain

A dependency is executable code: `build.rs` scripts and proc macros run
arbitrary code on your machine and in CI the moment you build. CI therefore
gates on [cargo-deny](https://crates.io/crates/cargo-deny):

```sh
cargo install cargo-deny --locked
cargo deny check advisories bans sources
```

Three checks run, with the policy and the reasoning behind each in `deny.toml`:

- **advisories** — RustSec vulnerability reports, and **yanked** crates. A yank
  is how a compromised release is withdrawn from crates.io, so a yanked version
  in `Cargo.lock` fails the build.
- **bans** — wildcard version requirements, which accept any future release
  sight-unseen.
- **sources** — every crate must come from crates.io. A dependency that
  suddenly resolves to a git repository is the shape a takeover attack takes,
  and it would otherwise pass unnoticed in a lockfile diff.

If it fails on a *yanked* crate, the fix is usually
`cargo update -p <crate>@<version> --precise <newer>` — check for a non-yanked
release inside the same semver range first. If it fails on a real advisory with
no fix available, add an entry to `ignore` in `deny.toml` **with a reason and a
condition for revisiting it**. An entry there is a decision, not a TODO.

Two related rules apply to anything under `.github/`:

- **Actions are pinned to full commit SHAs**, with the version in a trailing
  comment (`uses: actions/checkout@d23441a… # v6`). A tag like `@v6` or
  `@master` is mutable, so whoever controls that repository can repoint it at
  new code that then runs with our token. Renovate keeps the digests current.
- **`persist-credentials: false` on every checkout** except the two Homebrew
  taps in `release.yml`, which push with those credentials. Without it the token
  is left in `.git/config`, where a build script compiled later in the same job
  can read it.

Dependency updates are batched by Renovate into a single weekly PR, and new
releases are quarantined for 7 days (`minimumReleaseAge`) so that a malicious
version has time to be caught and yanked before it reaches us. Security fixes
skip both the quarantine and the weekly window.
### Testing on Windows

Windows is where program resolution diverges most — `CreateProcessW` only ever
appends `.exe`, `PATH` is `;`-separated, and `:` appears inside every absolute
path. Issues #449, #450 and #451 all lived there and none were reachable from a
Linux runner. CI now has a `windows-latest` job, but a local VM makes the loop
minutes instead of a CI round trip.

**Write the test so it does not need Windows where possible.** Several
invariants hold on every platform and should be asserted everywhere — the
`PATH` round-trip through `split_paths` in `path_env.rs` and the argv
pass-through in `runner.rs` both fail on macOS if the fix regresses. Reserve
`#[cfg(windows)]` for assertions only Windows can observe, such as a drive
letter surviving as a single `PATH` entry.

#### Setting up a disposable VM

Windows evaluation images expire after 90 days, so treat the VM as disposable
and re-create it when it lapses — that is Microsoft's intended path, not a
workaround. On Apple Silicon you need an **ARM64** build; the ordinary x64 ISO
will not install.

```sh
brew install --cask utm crystalfetch   # virtualiser + official ARM64 ISO downloader
```

1. Use **CrystalFetch** to build a Windows 11 ARM64 ISO. It assembles one from
   Microsoft's official UUP update packages via UUP Dump — the payload is
   Microsoft's, the tooling that fetches and assembles it is third-party. If you
   would rather avoid that, Microsoft publishes ARM64 VHDX images to Windows
   Insider members, and VMware Fusion (free, but behind a Broadcom account) can
   download Windows 11 ARM directly.
2. Create a UTM VM from that ISO (Virtualise, not Emulate — Apple's hypervisor
   runs ARM64 guests at native speed). Give it 4+ CPUs, 8 GB RAM and 80 GB disk;
   the first build compiles Luau, SQLite and aws-lc from source.
3. In an **elevated** PowerShell inside the VM:

   ```powershell
   iwr -useb https://raw.githubusercontent.com/mpecan/tokf/main/scripts/provision-windows-dev.ps1 | iex
   ```

   That installs Git, rustup (honouring `rust-toolchain.toml`), Visual Studio
   Build Tools with the C++ workload, CMake and NASM, then clones the repo.
4. **Snapshot the VM once the first build succeeds.** That snapshot is what you
   return to — both after a bad experiment and when the licence lapses.

Then run what CI runs:

```powershell
cargo clippy --locked -p tokf --all-targets -- -D warnings
cargo test   --locked -p tokf --lib
cargo test   --locked -p tokf --bin tokf
```

Two caveats worth knowing:

- A VM on Apple Silicon is **ARM64**, while CI runs **x86-64**. For OS-level
  behaviour — `PATHEXT`, path separators, `CreateProcess` semantics — the two
  are equivalent, which covers this whole bug class. Anything architecture-
  specific still only shows up in CI.
- `aws-lc-sys` is the most fragile dependency to build on Windows, ARM64
  especially. If it fails, that is a toolchain problem in the VM (usually a
  missing C++ workload or CMake), not a problem with your change.

#### What cross-compiling from macOS cannot do

`cargo check --target x86_64-pc-windows-msvc` does **not** work as a shortcut:
`libsqlite3-sys`, `mlua-sys` and `aws-lc-sys` all run build scripts that need a
Windows C/C++ toolchain. `cross` does not help on Apple Silicon either — its
images are x86-64 and the host cannot install the matching toolchain. The VM
and CI are the two real options.

### Writing an isolated test

tokf keeps its runtime configuration **explicit**. User directories, the
tracking database path, debug and telemetry flags all live in a `Runtime`
value that `main()` builds once and passes down. There are no globals to
override, so tests construct the environment they want instead of mutating a
shared one.

(Two things remain ambient and are deliberately out of `Runtime`: other tools'
config locations resolved from `dirs::home_dir()` — `~/.claude`, `~/.codex` —
which `TOKF_HOME` has never governed, and clap's `#[arg(env = ...)]` flag
defaults. The latter must still be listed in `RUNTIME_ENV` in
`tests/common/mod.rs`; CI checks that.)

For unit tests and in-process integration tests, ask for an isolated runtime:

```rust
use tokf::runtime::Runtime;

#[test]
fn shims_land_in_the_configured_directory() {
    let rt = Runtime::isolated();          // fresh temp dir, all flags off
    generate_shims(&rt, &filters);
    assert!(rt.shims_dir().unwrap().join("git").exists());
}
```

`Runtime::isolated()` roots every path in its own temporary directory, removed
when the value drops, and takes a keyring service name unique to that instance.
Two of them never interact, so tests need no coordination — and no `#[serial]`.
`Runtime::default()` is the same thing, so a test that asks for nothing still
cannot touch your real `~/.config/tokf` or `tracking.db`.

Override individual fields with the builder:

```rust
let rt = Runtime::builder()
    .home(dir.path())
    .db_path(dir.path().join("custom.db"))
    .debug(true)
    .build();
```

Tests that spawn the **binary** are a separate case: a `Runtime` is an
in-process value and does not survive a process boundary, so isolation travels
as environment variables. Use the shared helper in `crates/tokf-cli/tests/common`,
which sets `TOKF_HOME` and `TOKF_DB_PATH` into a temp dir *and* clears every
other `TOKF_*` / `OTEL_*` variable your shell may have exported:

```rust
mod common;
use common::tokf;

#[test]
fn ls_lists_filters() {
    let output = tokf().arg("ls").output().unwrap();
    assert!(output.status.success());
}
```

Hold a `common::TestHome` when the test needs to seed config files, inspect
what the binary wrote, or run several commands against the same home.

`scripts/check-runtime-seam.sh` enforces all of this in CI: environment reads
are confined to `src/runtime/`, only `main()` calls `Runtime::from_env()`, and
integration tests may not spawn the binary except through `tests/common`.

Two rules follow from this:

- **Never add a new global** to hold configuration. Add a field to `Runtime`.
- **Never add `#[serial]`.** `serial_test` is deliberately not a dependency —
  it orders annotated tests against each other, not against every other test
  touching the same state, which is why it never actually fixed the flakiness
  it was hiding (see issue #429). If a test truly needs exclusive access to a
  shared *external* resource, raise it in review.

### Property tests for the rewrite engine

The shell rewrite engine has a property-based invariant suite in
`crates/tokf-cli/src/rewrite/proptest_rewrite.rs` (uses `proptest`, a dev-only
dependency). It generates bash from a narrow grammar (simple/piped/compound
commands, quoted args) and asserts structural invariants on the rewrite output:
`compound_segments` round-trips byte-for-byte, argv is never fabricated, the
output always parses, rewriting is idempotent, and quoted literals survive
intact. This class of bug (a rewrite that *looks* plausible but has a different
argv structure, e.g. #355's `head -1\necho` → `head -1echo`) slips past
example-based tests, so extend the grammar or add an invariant here when you
touch the engine. Run it with:

```sh
cargo test -p tokf proptest_rewrite
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

Use `tokf apply filters/my/filter.toml tests/fixtures/my_fixture.txt` to iterate quickly on a single fixture.

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

## Token estimator calibration

tokf estimates tokens from byte counts (see `crates/tokf-common/src/tokens.rs`). An optional
`tokenizer` cargo feature adds a real cl100k counter so that constant can be verified:

```sh
cargo test -p tokf --features tokenizer --test calibration -- --ignored --nocapture
```

The feature is **off by default and must stay that way**. It is for calibration only — nothing in
the runtime path may use it. In particular, never add `features = ["tokenizer"]` to any workspace
member's `tokf-common` dependency: cargo feature unification is additive, so one such entry would
drag the tokenizer's vocabulary build into every default build.

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
