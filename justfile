set dotenv-load

# Defaults (overridden by .env or environment)
CONTAINER_RUNTIME := env("CONTAINER_RUNTIME", "podman")
DATABASE_URL := env("DATABASE_URL", "postgresql://root@localhost:26257/tokf_test?sslmode=disable")
compose_file := "crates/tokf-server/docker-compose.yml"

# Run all checks
check: fmt-check lint test file-size

# Format code
fmt:
    cargo fmt

# Check formatting
fmt-check:
    cargo fmt -- --check

# Run unit tests (no database required)
test:
    cargo test

# Run clippy
lint:
    cargo clippy --workspace --all-targets -- -D warnings

# Check file sizes
file-size:
    bash scripts/check-file-sizes.sh

# Install the CLI
install:
    cargo install --path crates/tokf-cli

# Install the CLI (force)
force-install:
    cargo install --force --path crates/tokf-cli

# Generate README.md from docs/
readme:
    bash scripts/generate-readme.sh

# Verify README.md is up-to-date
readme-check:
    bash scripts/generate-readme.sh --check

# Install git hooks (run once after cloning)
install-hooks:
    chmod +x scripts/hooks/pre-commit
    ln -sf ../../scripts/hooks/pre-commit .git/hooks/pre-commit
    @echo "Git hooks installed."

# ── Database (CockroachDB) ────────────────────────────────────────────────────

# Start CockroachDB
db-start:
    {{ CONTAINER_RUNTIME }} compose -f {{ compose_file }} up -d

# Stop CockroachDB (preserves data)
db-stop:
    {{ CONTAINER_RUNTIME }} compose -f {{ compose_file }} down

# Reset CockroachDB (removes all data)
db-reset:
    {{ CONTAINER_RUNTIME }} compose -f {{ compose_file }} down -v
    {{ CONTAINER_RUNTIME }} compose -f {{ compose_file }} up -d

# Check if CockroachDB is running
db-status:
    @{{ CONTAINER_RUNTIME }} compose -f {{ compose_file }} ps 2>/dev/null || echo "CockroachDB is not running. Start it with: just db-start"

# Create the test database (idempotent)
db-setup:
    @psql "postgresql://root@localhost:26257/defaultdb?sslmode=disable" \
      -c "CREATE DATABASE IF NOT EXISTS tokf_test" 2>/dev/null \
      && echo "Database tokf_test is ready." \
      || echo "Could not connect to CockroachDB. Is it running? Try: just db-start"

# ── Integration & E2E tests ──────────────────────────────────────────────────

# Run tokf-server DB integration tests
test-db:
    cargo test -p tokf-server -- --ignored

# Run end-to-end tests
test-e2e:
    cargo test -p e2e-tests -- --ignored

# Run all tests: unit + DB integration + e2e
test-all: test test-db test-e2e
