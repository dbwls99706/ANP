# Multi-stage build for minimal image size
FROM rust:1.93-slim AS builder
RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/
RUN cargo build --release -p anp-cli

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/anp-cli /usr/local/bin/anp
COPY examples/ /app/examples/
WORKDIR /app
EXPOSE 8080
CMD ["anp", "run", "examples/fly_demo.anp.json"]
