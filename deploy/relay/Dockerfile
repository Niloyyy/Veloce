# Multi-stage build for veloce-relay (stateless blind E2EE UDP relay)
FROM rust:1.86-slim-bookworm AS builder

WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY core ./core
COPY relay ./relay
COPY cli ./cli
COPY bench ./bench

RUN cargo build --release --locked -p veloce-relay

FROM debian:bookworm-slim

RUN useradd --system --uid 10001 --no-create-home veloce
COPY --from=builder /src/target/release/veloce-relay /usr/local/bin/veloce-relay

USER veloce
ENV VELOCE_RELAY_BIND=0.0.0.0:7878
EXPOSE 7878/udp

ENTRYPOINT ["/usr/local/bin/veloce-relay"]
