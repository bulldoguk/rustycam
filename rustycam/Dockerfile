FROM rust:1.85-slim AS builder

WORKDIR /app
RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock* ./
COPY src ./src
COPY migrations ./migrations

RUN cargo build --release

# ---- runtime ----
FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y ffmpeg ca-certificates jq \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /app/target/release/rustycam /app/rustycam
COPY migrations ./migrations
COPY run.sh /app/run.sh
RUN chmod +x /app/run.sh

VOLUME ["/data"]
EXPOSE 8090

ENV RUST_LOG=rustycam=info

CMD ["/app/run.sh"]
