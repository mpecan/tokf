# ── Stage 1: cache dependencies ─────────────────────────────────────────────
# Copy manifests first so that source changes don't bust the dep-compile layer.
FROM rust:slim AS deps
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY crates/tokf-common/Cargo.toml crates/tokf-common/
COPY crates/tokf-cli/Cargo.toml    crates/tokf-cli/
COPY crates/tokf-server/Cargo.toml crates/tokf-server/
# Create empty source stubs so `cargo build` can resolve all dependencies.
RUN mkdir -p crates/tokf-common/src \
             crates/tokf-cli/src \
             crates/tokf-server/src && \
    echo 'fn main(){}' > crates/tokf-server/src/main.rs && \
    touch crates/tokf-common/src/lib.rs \
          crates/tokf-cli/src/lib.rs \
          crates/tokf-server/src/lib.rs && \
    cargo build --release -p tokf-server && \
    rm -rf crates/*/src

# ── Stage 2: build real source ───────────────────────────────────────────────
FROM deps AS builder
COPY crates/ crates/
# Touch to ensure Cargo detects the source change.
RUN touch crates/tokf-server/src/main.rs && \
    cargo build --release -p tokf-server

# ── Stage 3: minimal runtime image ───────────────────────────────────────────
FROM debian:bookworm-slim
RUN apt-get update && \
    apt-get install -y --no-install-recommends ca-certificates && \
    rm -rf /var/lib/apt/lists/* && \
    useradd -m -u 1000 tokf
WORKDIR /app
COPY --from=builder /app/target/release/tokf-server .
USER tokf
EXPOSE 8080
CMD ["./tokf-server"]
