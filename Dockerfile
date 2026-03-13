# ---- Build stage ----
FROM rust:1.94-bookworm AS builder

WORKDIR /build
COPY . .

RUN cargo build --release --workspace

# ---- Runtime stage ----
FROM debian:bookworm-slim AS runtime

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates libssl3 libpq5 \
    && rm -rf /var/lib/apt/lists/*

# Canonical persistent mount points (per DOCKER-DEPLOYMENT.md)
RUN mkdir -p /data/db /data/sessions /data/memory /data/media \
             /data/skills /data/logs /data/backups /config /secrets

COPY --from=builder /build/target/release/rune /usr/local/bin/rune
COPY --from=builder /build/target/release/rune-gateway /usr/local/bin/rune-gateway

EXPOSE 8787

VOLUME ["/data", "/config", "/secrets"]

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD rune health || exit 1

ENTRYPOINT ["rune-gateway"]
