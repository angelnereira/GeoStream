#!/usr/bin/env bash
# Levanta la imagen all-in-one y verifica que sirve de verdad.
# Uso: deploy/smoke.sh [puerto-host] [imagen]
# Sale 0 si /health/ready responde 200 y 1 en caso contrario.
set -euo pipefail

PORT="${1:-8090}"
IMAGE="${2:-geostream:all-in-one}"
NAME="geostream-smoke-$$"

cleanup() {
  docker rm -f "${NAME}" >/dev/null 2>&1 || true
}
trap cleanup EXIT

echo "[smoke] building ${IMAGE} (si hace falta)"
docker build -f "$(dirname "$0")/Dockerfile" -t "${IMAGE}" "$(dirname "$0")/.." >/dev/null

echo "[smoke] starting container ${NAME} on port ${PORT}"
docker run -d --name "${NAME}" -p "${PORT}:8080" "${IMAGE}" serve >/dev/null

echo "[smoke] waiting for readiness"
for _ in $(seq 1 40); do
  if curl -fsS "http://127.0.0.1:${PORT}/health/ready" >/dev/null 2>&1; then
    echo "[smoke] OK: /health/ready is 200"
    curl -sS "http://127.0.0.1:${PORT}/health/ready"; echo
    echo "[smoke] liveness:"
    curl -sS "http://127.0.0.1:${PORT}/health/live"; echo
    exit 0
  fi
  sleep 2
done

echo "[smoke] FAIL: service did not become ready in time" >&2
docker logs "${NAME}" >&2 || true
exit 1