#!/bin/sh
# Arranque en Railway. Los secretos se generan DENTRO del volumen /data la primera vez y nunca
# se imprimen: así no pasan por variables visibles, logs ni el repositorio (AGENTS.md).
set -eu

KEYS_DIR="${GS_JWT_KEYS_DIR:-/data/jwt-keys}"
KID="${GS_JWT_ACTIVE_KID:-prod-key-1}"
SECRETS_DIR=/data/secrets
umask 077

# 1. Par de firma Ed25519 de tokens de dispositivo (docs/10 §5).
mkdir -p "${KEYS_DIR}" "${SECRETS_DIR}"
if [ ! -f "${KEYS_DIR}/${KID}.key.pem" ]; then
  echo "[geostream] generating signing key pair for kid ${KID}"
  openssl genpkey -algorithm ed25519 -out "${KEYS_DIR}/${KID}.key.pem"
  openssl pkey -in "${KEYS_DIR}/${KID}.key.pem" -pubout -out "${KEYS_DIR}/${KID}.pub.pem"
fi

# 2. Clave AES-256-GCM de datos (GS_DATA_ENCRYPTION_KEY, docs/17 §2.4) si no viene por variable.
if [ -z "${GS_DATA_ENCRYPTION_KEY:-}" ]; then
  if [ ! -s "${SECRETS_DIR}/data-encryption.key" ]; then
    echo "[geostream] generating data encryption key"
    openssl rand -base64 32 | tr -d '\n' > "${SECRETS_DIR}/data-encryption.key"
  fi
  GS_DATA_ENCRYPTION_KEY="$(cat "${SECRETS_DIR}/data-encryption.key")"
  export GS_DATA_ENCRYPTION_KEY
fi

# 3. Migraciones (GS_AUTO_MIGRATE está prohibido en prod: se corren como paso explícito).
if [ "${1:-}" = "serve" ]; then
  echo "[geostream] running migrations"
  /usr/local/bin/geostream migrate
fi

exec /usr/local/bin/geostream "$@"
