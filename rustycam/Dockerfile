FROM rust:alpine AS builder

RUN apk add --no-cache musl-dev pkgconfig gcc

WORKDIR /app
COPY Cargo.toml Cargo.lock* ./
COPY src ./src
COPY migrations ./migrations

RUN cargo build --release

# ---- runtime ----
FROM alpine:3.21

RUN apk add --no-cache ffmpeg ca-certificates jq bash

WORKDIR /app
COPY --from=builder /app/target/release/rustycam /app/rustycam
COPY migrations ./migrations
COPY run.sh /app/run.sh
RUN chmod +x /app/run.sh

VOLUME ["/data"]
EXPOSE 8090

ENV RUST_LOG=rustycam=info

CMD ["/app/run.sh"]
