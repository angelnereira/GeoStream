# GeoStream — imagen UNICA que empaqueta PostgreSQL, Valkey y el binario
# `geostream` en un solo contenedor. arranca todo en `entrypoint.sh`.
# Build desde la raiz del repo:
#   docker build -f deploy/Dockerfile -t geostream:all-in-one .

# --- Stage 1: compilacion del binario ---
FROM rust:1.99-slim-bookworm AS builder
WORKDIR /src
RUN apt-get update && apt-get install -y --no-install-recommends pkg-config libssl-dev ca-certificates \
 && rm -rf /var/lib/apt/lists/*
COPY . .
RUN cargo build --release -p geostream

# --- Stage 2: imagen all-in-one ---
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
      postgresql postgresql-client redis-server openssl ca-certificates curl tini \
 && rm -rf /var/lib/apt/lists/* \
 && useradd -r -u 10001 -m geostream \
 && mkdir -p /data/pgdata /data/jwt-keys /data/config \
 && chown -R postgres:postgres /data/pgdata \
 && chown -R geostream:geostream /data/jwt-keys /data/config

COPY --from=builder /src/target/release/geostream /usr/local/bin/geostream
COPY deploy/entrypoint.sh /usr/local/bin/geostream-entrypoint
RUN chmod +x /usr/local/bin/geostream-entrypoint

ENV GS_HTTP_ADDR=0.0.0.0:8080 \
    GS_DATABASE_URL=postgres://geostream:geostream_dev_password@127.0.0.1:5432/geostream_dev \
    GS_AUTO_MIGRATE=true \
    GS_JWT_KEYS_DIR=/data/jwt-keys \
    GS_JWT_ACTIVE_KID=dev-key-1 \
    GS_JWT_ISSUER=https://geostream.local \
    # en single-container PG/Valkey van en 127.0.0.1
    GS_REDIS_URL=redis://127.0.0.1:6379 \
    RUST_LOG=info
EXPOSE 8080
HEALTHCHECK --interval=5s --timeout=3s --retries=20 CMD curl -fsS http://127.0.0.1:8080/health/ready >/dev/null || exit 1
ENTRYPOINT ["/usr/local/bin/geostream-entrypoint"]
CMD ["serve"]