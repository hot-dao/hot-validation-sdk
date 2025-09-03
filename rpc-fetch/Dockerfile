FROM rust:1-bookworm AS builder
WORKDIR /app
RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*
COPY . .
RUN cargo build --release -p hot-validation-rpc-fetch

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/hot-validation-rpc-fetch /usr/local/bin/hot-validation-rpc-fetch
ENV RUST_LOG=INFO
ENV RUST_BACKTRACE=0
ENTRYPOINT ["/usr/local/bin/hot-validation-rpc-fetch"]