# Run all checks
check: fmt-check lint test file-size

# Format code
fmt:
    cargo fmt

# Check formatting
fmt-check:
    cargo fmt -- --check

# Run tests
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

# Install the CLI
force-install:
    cargo install --force --path crates/tokf-cli

# Install git hooks (run once after cloning)
install-hooks:
    chmod +x scripts/hooks/pre-commit
    ln -sf ../../scripts/hooks/pre-commit .git/hooks/pre-commit
    @echo "Git hooks installed."
