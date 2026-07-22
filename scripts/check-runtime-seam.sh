#!/usr/bin/env bash
#
# Guards the boundaries that keep tokf's runtime configuration explicit
# (issue #429). Each check below exists because violating it silently
# reintroduces ambient global state, which cost us two shipped bugs (#422).
#
# 1. The environment is read in exactly one place.
# 2. Only main() builds a Runtime from the environment.
# 3. Integration tests spawn the binary only through the isolating helper.
# 4. serial_test stays gone, so no test can paper over shared state again.

set -euo pipefail

exit_code=0

fail() {
    echo "ERROR: $1"
    exit_code=1
}

# --- 1. Environment reads are confined to the runtime module ---------------
#
# `Runtime::from_env()` is the single seam. Anything else reading TOKF_* or
# OTEL_* at the point of use is exactly the ambient-state pattern we removed.
env_reads=$(grep -rn --include='*.rs' \
    -E 'env::var(_os)?\("(TOKF_|OTEL_)' \
    crates/tokf-cli/src \
    | grep -v '^crates/tokf-cli/src/runtime/' || true)

if [ -n "$env_reads" ]; then
    fail "TOKF_*/OTEL_* environment reads outside crates/tokf-cli/src/runtime/:"
    echo "$env_reads" | sed 's/^/  /'
    echo "  Add the value to Runtime instead, and pass &Runtime to the caller."
fi

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
    echo "$from_env" | sed 's/^/  /'
    echo "  Accept a &Runtime parameter, or use Runtime::isolated() in tests."
fi

# --- 3. Integration tests spawn the binary only via tests/common ----------
#
# A Runtime does not cross a process boundary, so isolation for spawned
# binaries travels as TOKF_HOME / TOKF_DB_PATH. tests/common sets those and
# clears every other runtime variable; bypassing it means the test runs
# against the developer's real config directory.
raw_spawn=$(grep -rn --include='*.rs' \
    'Command::new(env!("CARGO_BIN_EXE_tokf"))' \
    crates/tokf-cli/tests \
    | grep -v '^crates/tokf-cli/tests/common/' || true)

if [ -n "$raw_spawn" ]; then
    fail "integration tests spawning tokf directly instead of via tests/common:"
    echo "$raw_spawn" | sed 's/^/  /'
    echo "  Use common::tokf() or common::TestHome::new().cmd()."
fi

# --- 4. serial_test stays removed -----------------------------------------
#
# 52 #[serial] annotations existed only to paper over the process-global
# state that is now gone. Keeping the crate out means a new one cannot compile.
if grep -q '^serial_test' crates/*/Cargo.toml 2>/dev/null; then
    fail "serial_test is back in a Cargo.toml."
    echo "  #[serial] hides shared state rather than removing it — see issue #429."
    echo "  If a test genuinely needs a shared external resource, say so here."
fi

if [ "$exit_code" -eq 0 ]; then
    echo "Runtime seam check passed."
fi

exit $exit_code
