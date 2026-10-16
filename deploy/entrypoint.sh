#!/bin/sh
set -eu

DATA_PG=/data/pgdata
KEYS_DIR="${GS_JWT_KEYS_DIR:-/data/jwt-keys}"
KID="${GS_JWT_ACTIVE_KID:-dev-key-1}"
PRIV="${KEYS_DIR}/${KID}.key.pem"
PUB="${KEYS_DIR}/${KID}.pub.pem"

# --- 1. PostgreSQL en este mismo contenedor ---
if [ ! -s "${DATA_PG}/PG_VERSION" ]; then
  echo "[geostream] inicializando cluster PostgreSQL"
  rm -rf "${DATA_PG}"
  mkdir -p "${DATA_PG}"
  chown postgres:postgres "${DATA_PG}"
  su postgres -c "/usr/lib/postgresql/15/bin/initdb -D '${DATA_PG}' >/tmp/initdb.log 2>&1"
  echo "host all all 0.0.0.0/0 md5" >> "${DATA_PG}/pg_hba.conf"
  sed -i "s/#listen_addresses = 'localhost'/listen_addresses = '*'/ " "${DATA_PG}/postgresql.conf"
fi

echo "[geostream] arrancando PostgreSQL"
su postgres -c "/usr/lib/postgresql/15/bin/pg_ctl -D '${DATA_PG}' -l /tmp/pg.log start -w"
# espera socket
for i in $(seq 1 30); do
  if su postgres -c "pg_isready -q" 2>/dev/null; then break; fi
  sleep 1
done
su postgres -c "psql -v ON_ERROR_STOP=1 -c \"CREATE USER geostream WITH PASSWORD 'geostream_dev_password';\" 2>/dev/null || true"
su postgres -c "psql -v ON_ERROR_STOP=1 -c \"CREATE DATABASE geostream_dev OWNER geostream;\" 2>/dev/null || true"

# --- 2. Valkey ---
echo "[geostream] arrancando Redis"
redis-server --port 6379 --daemonize yes --save "" --appendonly no
for i in $(seq 1 20); do
  if valkey-cli ping >/dev/null 2>&1; then break; fi
  sleep 0.5
done

# --- 3. Claves JWT dev ---
if [ ! -f "${PRIV}" ]; then
  echo "[geostream] generando par de firma dev ${KID}"
  mkdir -p "${KEYS_DIR}"
  openssl genpkey -algorithm ed25519 -out "${PRIV}"
  openssl pkey -in "${PRIV}" -pubout -out "${PUB}"
  chmod 600 "${PRIV}"
fi

echo "[geostream] exec: $*"
exec /usr/local/bin/geostream "$@"