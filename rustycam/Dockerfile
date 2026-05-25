FROM rust:bookworm AS builder

RUN apt-get update && apt-get install -y musl-tools && rm -rf /var/lib/apt/lists/*
RUN rustup target add x86_64-unknown-linux-musl

WORKDIR /app
COPY Cargo.toml Cargo.lock* ./
COPY src ./src
COPY migrations ./migrations

RUN cargo build --release --target x86_64-unknown-linux-musl

# ---- runtime ----
FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y ffmpeg ca-certificates jq \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/rustycam /app/rustycam
COPY migrations ./migrations
COPY run.sh /app/run.sh
RUN chmod +x /app/run.sh

VOLUME ["/data"]
EXPOSE 8090

ENV RUST_LOG=rustycam=info

CMD ["/app/run.sh"]
