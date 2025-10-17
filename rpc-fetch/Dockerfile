########################
# 1) Cargo-chef base
########################
FROM rust:1-bookworm AS chef
WORKDIR /app
RUN cargo install cargo-chef

########################
# 2) Planner (deps recipe)
########################
FROM chef AS planner
COPY . .
RUN cargo chef prepare \
  --bin hot-validation-rpc-fetch \
  --recipe-path recipe.json

########################
# 3) Builder
########################
FROM chef AS builder
WORKDIR /app

# Toolchain & native deps
RUN apt-get update -y \
 && apt-get install -y --no-install-recommends \
      pkg-config libssl-dev build-essential cmake clang libclang-dev \
 && rm -rf /var/lib/apt/lists/*

# Compiler cache
RUN cargo install sccache
ENV RUSTC_WRAPPER=/usr/local/cargo/bin/sccache \
    SCCACHE_DIR=/sccache \
    CARGO_TERM_COLOR=always

# Bring deps recipe
COPY --from=planner /app/recipe.json /app/recipe.json

# Cook dependencies (release) with aggressive caches
RUN --mount=type=cache,target=/root/.cargo/registry \
    --mount=type=cache,target=/root/.cargo/git \
    --mount=type=cache,target=/app/target \
    --mount=type=cache,target=/sccache \
    cargo chef cook \
      --package hot-validation-rpc-fetch \
      --release \
      --recipe-path /app/recipe.json

# Now sources and final build
COPY . .

# Final build uses caches, then persists the binary to /out
RUN --mount=type=cache,target=/root/.cargo/registry \
    --mount=type=cache,target=/root/.cargo/git \
    --mount=type=cache,target=/app/target \
    --mount=type=cache,target=/sccache \
    bash -c '\
      set -euo pipefail; \
      echo "== Build hot-validation-rpc-fetch (release) =="; \
      cargo build --release --package hot-validation-rpc-fetch --locked; \
      mkdir -p /out; \
      cp target/release/hot-validation-rpc-fetch /out/; \
      ls -l /out \
    '

########################
# 4) Runtime
########################
FROM debian:bookworm-slim
RUN apt-get update -y \
 && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/*
COPY --from=builder /out/hot-validation-rpc-fetch /usr/local/bin/hot-validation-rpc-fetch
ENTRYPOINT ["/usr/local/bin/hot-validation-rpc-fetch"]
