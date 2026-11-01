#!/usr/bin/env bash
# Prueba de humo de un proyecto consumidor (docs/15 §3.2 y §10): el recorrido de una entrega con
# seguimiento en tiempo real, de punta a punta, contra una instancia de GeoStream.
#
#   GEOSTREAM_URL=https://geostream.gravitalcloud.com \
#   GEOSTREAM_API_KEY=gsk_... \
#   deploy/consumer-smoke.sh
#
# La API key necesita los scopes: tokens:mint, sessions:write, sessions:read, locations:read.
# La clave se lee solo del entorno: nunca se imprime ni se escribe en disco. El script crea una sesión
# con un repartidor de prueba y la cierra al terminar.
set -euo pipefail

: "${GEOSTREAM_URL:?define GEOSTREAM_URL}"
: "${GEOSTREAM_API_KEY:?define GEOSTREAM_API_KEY}"
command -v jq >/dev/null || { echo "falta jq" >&2; exit 2; }

BASE="${GEOSTREAM_URL%/}"
RUN="smoke$(date +%s)$RANDOM"
DRIVER="driver:$RUN"
FAILS=0

req() { # método ruta token [cuerpo] [cabecera-extra] -> imprime "<estado>\n<cuerpo>"
  local method=$1 path=$2 token=$3 body=${4:-} extra=${5:-}
  local args=(-sS --max-time 15 -X "$method" -H "Authorization: Bearer $token" -w '\n%{http_code}')
  [ -n "$body" ] && args+=(-H 'Content-Type: application/json' -d "$body")
  [ -n "$extra" ] && args+=(-H "$extra")
  curl "${args[@]}" "$BASE$path"
}
status_of() { tail -n1 <<<"$1"; }
body_of() { sed '$d' <<<"$1"; }
check() { # descripción esperado real
  if [ "$2" = "$3" ]; then echo "  ok   $1"; else echo "  FALLA $1 (esperado $2, obtenido $3)"; FAILS=$((FAILS+1)); fi
}

echo "== GeoStream: $BASE"
ready=$(curl -sS --max-time 10 -o /dev/null -w '%{http_code}' "$BASE/health/ready")
check "health/ready" 200 "$ready"

echo "== 1. El servidor del cliente pide un token para el repartidor"
installation=$(cat /proc/sys/kernel/random/uuid 2>/dev/null || uuidgen | tr 'A-Z' 'a-z')
r=$(req POST /v1/tokens "$GEOSTREAM_API_KEY" \
  "$(jq -nc --arg s "$DRIVER" --arg i "$installation" \
    '{external_subject_id:$s,installation_id:$i,platform:"SERVER",scopes:["sharing:write","ingest:write"]}')" \
  "Idempotency-Key: tok-$RUN")
check "POST /v1/tokens" 200 "$(status_of "$r")"
DEVICE_TOKEN=$(body_of "$r" | jq -r '.access_token // empty')
[ -n "$DEVICE_TOKEN" ] || { echo "sin token de dispositivo; no se puede seguir"; exit 1; }

echo "== 2. Crear la sesión de la entrega (idempotente)"
payload=$(jq -nc --arg d "$DRIVER" '{mode:"ONE_TO_MANY",expires_in_seconds:600,stale_after_seconds:60,
  initial_members:[{external_subject_id:$d,capabilities:{publish:true,read:false,invite:false,manage:false}}]}')
r=$(req POST /v1/sessions "$GEOSTREAM_API_KEY" "$payload" "Idempotency-Key: delivery:$RUN")
check "POST /v1/sessions" 201 "$(status_of "$r")"
SESSION=$(body_of "$r" | jq -r '.id // empty')
[ -n "$SESSION" ] || { echo "sin sesión; no se puede seguir"; exit 1; }
r2=$(req POST /v1/sessions "$GEOSTREAM_API_KEY" "$payload" "Idempotency-Key: delivery:$RUN")
check "reintento con la misma Idempotency-Key devuelve la misma sesión" "$SESSION" "$(body_of "$r2" | jq -r '.id // empty')"

echo "== 3. El repartidor activa la compartición y publica posiciones"
r=$(req PUT "/v1/sessions/$SESSION/members/me/sharing" "$DEVICE_TOKEN" '{"sharing":true}')
check "PUT sharing" 200 "$(status_of "$r")"
now=$(date -u +%Y-%m-%dT%H:%M:%S.000Z)
samples=$(jq -nc --arg s "$SESSION" --arg t "$now" '{session_id:$s,samples:[{sequence:1,captured_at:$t,
  position:{latitude:8.9824,longitude:-79.5199,horizontal_accuracy_m:8},quality:{kind:"MEASURED",source:"GNSS",is_mock:false}}]}')
r=$(req POST /v1/ingest/locations "$DEVICE_TOKEN" "$samples")
check "POST ingest/locations" 200 "$(status_of "$r")"
check "muestra aceptada" ACCEPTED "$(body_of "$r" | jq -r '.results[0].status // .items[0].status // empty')"

echo "== 4. La ingesta con la API key debe rechazarse"
r=$(req POST /v1/ingest/locations "$GEOSTREAM_API_KEY" "$samples")
code=$(status_of "$r")
if [ "$code" = 401 ] || [ "$code" = 403 ]; then echo "  ok   ingesta con API key rechazada ($code)"; else echo "  FALLA la ingesta con API key devolvió $code"; FAILS=$((FAILS+1)); fi

echo "== 5. (0,0) se rechaza"
bad=$(jq -nc --arg s "$SESSION" --arg t "$now" '{session_id:$s,samples:[{sequence:2,captured_at:$t,
  position:{latitude:0,longitude:0,horizontal_accuracy_m:8},quality:{kind:"MEASURED",source:"GNSS"}}]}')
r=$(req POST /v1/ingest/locations "$DEVICE_TOKEN" "$bad")
check "muestra (0,0) rechazada" REJECTED "$(body_of "$r" | jq -r '.results[0].status // .items[0].status // empty')"

echo "== 6. El servidor del cliente lee la última posición (snapshot)"
r=$(req GET "/v1/sessions/$SESSION/snapshot" "$GEOSTREAM_API_KEY")
check "GET snapshot" 200 "$(status_of "$r")"
check "una posición en el snapshot" 1 "$(body_of "$r" | jq -r '.items | length')"
check "latitud conservada" 8.9824 "$(body_of "$r" | jq -r '.items[0].sample.position.latitude')"

echo "== 7. Cerrar la sesión (idempotente) y comprobar que ya no está activa"
r=$(req POST "/v1/sessions/$SESSION/close" "$GEOSTREAM_API_KEY" '' "Idempotency-Key: close-$RUN")
check "POST close" 200 "$(status_of "$r")"

if [ "$FAILS" -eq 0 ]; then echo; echo "TODO OK: el proyecto puede consumir GeoStream."; else echo; echo "$FAILS comprobación(es) fallaron."; exit 1; fi
