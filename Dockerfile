# syntax=docker/dockerfile:1
# ── Stage 1: build ──────────────────────────────────────────────────────────
FROM rust:slim-bookworm AS builder
# g++ is required for mlua's vendored Luau build (C++ source compiled via cc crate)
RUN apt-get update && apt-get install -y --no-install-recommends g++ && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/app/target \
    cargo build --release -p tokf-server && \
    cp target/release/tokf-server /app/tokf-server-bin

# ── Stage 2: minimal runtime image ─────────────────────────────────────────
FROM debian:bookworm-slim
RUN apt-get update && \
    apt-get install -y --no-install-recommends ca-certificates && \
    rm -rf /var/lib/apt/lists/* && \
    useradd -m -u 1000 tokf
WORKDIR /app
COPY --from=builder /app/tokf-server-bin ./tokf-server
USER tokf
EXPOSE 8080
CMD ["./tokf-server"]
