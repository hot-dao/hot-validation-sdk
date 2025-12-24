# syntax=docker/dockerfile:1.7
# 🧩 This Dockerfile uses cargo-chef for dependency caching
# and sccache for compiler-level caching.
# Works great with BuildKit and docker compose build.

########################
# 1️⃣ Cargo Chef Base
########################
FROM rust:1-bookworm AS chef
RUN cargo install cargo-chef
WORKDIR /app

########################
# 2️⃣ Planner Stage
# Generates the dependency recipe
########################
FROM chef AS planner
COPY . .
RUN cargo chef prepare \
    --bin hot-protocol-mpc-proxy \
    --recipe-path recipe.json

########################
# 3️⃣ Builder Stage
########################
FROM chef AS builder

# Optional build arguments
ARG FEATURES=""
ARG RELEASE_PROFILE=""

# 🧰 Install build deps
RUN apt-get update -y \
 && apt-get install -y --no-install-recommends \
    build-essential \
    cmake \
    clang \
    libclang-dev \
 && rm -rf /var/lib/apt/lists/*

# ⚡ Install sccache to speed up Rust compilation
RUN cargo install sccache
ENV RUSTC_WRAPPER=/usr/local/cargo/bin/sccache \
    SCCACHE_DIR=/sccache \
    CARGO_TERM_COLOR=always

WORKDIR /app

# Copy dependency plan from the planner
COPY --from=planner /app/recipe.json recipe.json

# 🧱 Build dependency layers
# These layers will be reused if Cargo.toml/Cargo.lock didn't change
RUN --mount=type=cache,target=/root/.cargo/registry \
    --mount=type=cache,target=/root/.cargo/git \
    --mount=type=cache,target=/app/target \
    --mount=type=cache,target=/sccache \
    cargo chef cook \
      --package hot-protocol-mpc-proxy \
      ${RELEASE_PROFILE:+--release} \
      ${FEATURES:+--features "$FEATURES"} \
      --locked \
      --recipe-path recipe.json

# 📦 Copy full project source and build the final binary
COPY . .

# ✅ Final build: keep target cache for incremental rebuilds,
#    but persist the binary to /out so the next stage can COPY it.
# NOTE: use 'bash -c' (NOT '-l'); login shells can drop cargo from PATH.
RUN --mount=type=cache,target=/root/.cargo/registry \
    --mount=type=cache,target=/root/.cargo/git \
    --mount=type=cache,target=/app/target \
    --mount=type=cache,target=/sccache \
    bash -c '\
      set -euo pipefail; \
      echo "======================================"; \
      echo "🔨 cargo build (${RELEASE_PROFILE:+release}${RELEASE_PROFILE:-debug}) features: ${FEATURES:-<none>}"; \
      echo "PATH=$PATH"; command -v cargo; \
      echo "======================================"; \
      cargo build \
        --package hot-protocol-mpc-proxy \
        ${RELEASE_PROFILE:+--release} \
        ${FEATURES:+--features "$FEATURES"} \
        --locked; \
      mkdir -p /out; \
      cp target/${RELEASE_PROFILE:+release}${RELEASE_PROFILE:-debug}/hot-protocol-mpc-proxy /out/hot-protocol-mpc-proxy; \
      ls -l /out \
    '

########################
# 4️⃣ Runtime Stage
########################
FROM debian:bookworm-slim AS runtime

# Minimal runtime dependencies
RUN apt-get update -y \
    && apt-get install -y --no-install-recommends openssl ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Arguments for choosing build output (info-only)
ARG RELEASE_PROFILE=""
ARG FEATURES=""

# ℹ️ Output info at build-time
RUN echo "======================================" && \
    echo "🧱 Shipping profile: ${RELEASE_PROFILE:-debug}" && \
    echo "⚙️  Enabled features: ${FEATURES:-<none>}" && \
    echo "======================================"

# Copy binary from a stable path exported by the builder (avoids target/ cache mount visibility issues)
COPY --from=builder /out/hot-protocol-mpc-proxy /usr/local/bin/hot-protocol-mpc-proxy

# ✅ Final entrypoint
ENTRYPOINT ["/usr/local/bin/hot-protocol-mpc-proxy"]
