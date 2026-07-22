#!/usr/bin/env bash
#
# Guards the boundaries that keep tokf's runtime configuration explicit
# (issue #429). Each check below exists because violating it silently
# reintroduces ambient global state, which cost us two shipped bugs (#422).
#
# 1.  The environment is read in exactly one place.
# 1a. No test mutates the process environment.
# 1b. Reads that dodge the literal-string check are still cleared by the
#     integration-test harness.
# 2.  Only main() builds a Runtime from the environment.
# 3.  Integration tests spawn the binary only through the isolating helper.
# 4.  serial_test stays gone, so no test can paper over shared state again.

set -euo pipefail

# Every check greps repo-relative paths and filters on `^crates/...`, so this
# must run from the repo root. Without the cd, running it from anywhere else
# makes grep fail, `|| true` swallows the error, and the script reports success.
cd "$(dirname "$0")/.."

exit_code=0

fail() {
    echo "ERROR: $1" >&2
    exit_code=1
}

detail() {
    sed 's/^/  /' >&2
}

hint() {
    echo "  $1" >&2
}

# --- 1. Environment reads are confined to the runtime module ---------------
#
# `Runtime::from_env()` is the single seam. Anything else reading TOKF_* or
# OTEL_* at the point of use is exactly the ambient-state pattern we removed.
env_reads=$(grep -rn --include='*.rs' \
    -E 'env::var(_os)?\("(TOKF_|OTEL_)' \
    crates/tokf-cli/src crates/tokf-cli/tests crates/e2e-tests \
    | grep -v '^crates/tokf-cli/src/runtime/' || true)

if [ -n "$env_reads" ]; then
    fail "TOKF_*/OTEL_* environment reads outside crates/tokf-cli/src/runtime/:"
    echo "$env_reads" | detail
    hint "Add the value to Runtime instead, and pass &Runtime to the caller."
fi

# --- 1a. No test may mutate the process environment ------------------------
#
# `unsafe { env::set_var("TOKF_HOME", ...) }` is the original #422 pattern: it
# changes state every other test in the binary can observe, and `#[serial]`
# does not fix it because it only orders the annotated tests against each
# other. Tests build a Runtime instead, or set variables on a child process.
set_var=$(grep -rn --include='*.rs' \
    -E 'env::(set_var|remove_var)\("(TOKF_|OTEL_)' \
    crates/tokf-cli crates/e2e-tests || true)

if [ -n "$set_var" ]; then
    fail "code mutating the process environment:"
    echo "$set_var" | detail
    hint "Build a Runtime instead, or set the variable on the child Command."
fi

# --- 1b. Reads that dodge the literal-string check -------------------------
#
# Two shapes slip past check 1, and both were found escaping the test harness
# in review: a clap `#[arg(env = "TOKF_...")]` attribute, and
# `env::var(SOME_CONST)` where the name lives in a const. Neither is
# forbidden — clap's `env` is a legitimate way to expose a flag — but every
# such name MUST be listed in RUNTIME_ENV in tests/common/mod.rs, or the
# integration harness cannot clear it and a developer's exported value leaks
# into the suite. TOKF_PRESERVE_COLOR did exactly that, failing three tests.
harness="crates/tokf-cli/tests/common/mod.rs"

clap_env=$(grep -rho --include='*.rs' -E 'env = "(TOKF|OTEL)_[A-Z_]+"' \
    crates/tokf-cli/src \
    | sed -E 's/.*"([A-Z_]+)"/\1/' | sort -u || true)

const_names=$(grep -rho --include='*.rs' -E 'const [A-Z_]+: &str = "(TOKF|OTEL)_[A-Z_]+"' \
    crates/tokf-cli/src \
    | sed -E 's/.*"([A-Z_]+)"/\1/' | sort -u || true)

for name in $clap_env $const_names; do
    if ! grep -q "\"$name\"" "$harness"; then
        fail "$name is read from the environment but the test harness never clears it."
        hint "Add \"$name\" to RUNTIME_ENV in $harness."
    fi
done

# --- 2. Only main() constructs a Runtime from the environment --------------
#
# Everywhere else must receive one. In tests, Runtime::isolated() (or
# Runtime::default()) gives a private temporary directory instead.
from_env=$(grep -rn --include='*.rs' 'Runtime::from_env()' \
    crates/tokf-cli/src \
    | grep -v '^crates/tokf-cli/src/runtime/' \
    | grep -v '^crates/tokf-cli/src/main.rs:' || true)

if [ -n "$from_env" ]; then
    fail "Runtime::from_env() called outside main.rs:"
    echo "$from_env" | detail
    hint "Accept a &Runtime parameter, or use Runtime::isolated() in tests."
fi

# --- 3. Integration tests spawn the binary only via tests/common ----------
#
# A Runtime does not cross a process boundary, so isolation for spawned
# binaries travels as TOKF_HOME / TOKF_DB_PATH. tests/common sets those and
# clears every other runtime variable; bypassing it means the test runs
# against the developer's real config directory.
#
# Matches the direct form and the "hide it behind a helper" form, e.g.
# `const fn tokf_path() -> &'static str { env!("CARGO_BIN_EXE_tokf") }`.
raw_spawn=$(grep -rn --include='*.rs' \
    -E 'Command::new\((env!\("CARGO_BIN_EXE_tokf"\)|tokf_path\(\))' \
    crates/tokf-cli/tests crates/e2e-tests \
    | grep -v '^crates/tokf-cli/tests/common/' || true)

if [ -n "$raw_spawn" ]; then
    fail "integration tests spawning tokf directly instead of via tests/common:"
    echo "$raw_spawn" | detail
    hint "Use common::tokf() or common::TestHome::new().cmd()."
fi

# --- 4. serial_test stays removed -----------------------------------------
#
# 52 #[serial] annotations existed only to paper over the process-global
# state that is now gone. Keeping the crate out means a new one cannot compile.
if grep -q '^serial_test' Cargo.toml crates/*/Cargo.toml 2>/dev/null; then
    fail "serial_test is back in a Cargo.toml."
    hint "#[serial] hides shared state rather than removing it — see issue #429."
    hint "If a test genuinely needs a shared external resource, say so in review."
fi

if [ "$exit_code" -eq 0 ]; then
    echo "Runtime seam check passed."
fi

exit $exit_code
