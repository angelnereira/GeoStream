# GeoStream — Proxy TLS de referencia (NFR-SEC-001, docs/16 §2.3)

Esta configuración es la terminación TLS de referencia para GeoStream. El servidor escucha en
HTTP sin TLS (`GS_HTTP_ADDR`, por defecto `0.0.0.0:8080`) y en el listener de administración
(`GS_ADMIN_ADDR`, por defecto `127.0.0.1:9090`); el proxy añade TLS 1.2+, HSTS y el soporte
de WebSocket.

Requisitos normativos que cumple (`docs/16_OPERATIONS.md` §2.3):

| Requisito | Dónde se ve |
|---|---|
| Terminación TLS 1.2+ | `tls` con `protocols tls1.2 tls1.3` (Caddy); `ssl_protocols TLSv1.2 TLSv1.3` (Nginx) |
| HSTS en despliegues públicos | `Strict-Transport-Security` |
| Soporte de WebSocket (HTTP/1.1 Upgrade) | `@websocket` + `header_up Upgrade/Connection` |
| Timeout de inactividad > `GS_STREAM_IDLE_TIMEOUT_MS` + margen (≥ 90 s) | `read_timeout 3600` / `proxy_read_timeout 3600` |
| Límite de cuerpo ≥ `GS_REST_MAX_BODY_BYTES` | `request_body max_size` (Caddy usa MB) |
| `X-Forwarded-For` / `X-Forwarded-Proto` | `header_up X-Forwarded-For/Proto`; el servidor solo los confía con `GS_TRUST_PROXY_HEADERS=true` y `GS_TRUSTED_PROXY_CIDRS` |
| Sin afinidad de sesión | no hay `sticky` ni `ip_hash` |
| Panel de administración aislado | el listener de administración no se monta tras el proxy público |

El listener de administración (`/metrics`, copia de `/health/*`) NO debe exponerse al público:
se monta en una interfaz interna o se alcanza por un túnel. `deploy/docker-compose.yml` de
referencia (T-F6-05) mostrará las dos interfaces separadas.

## Caddy (`deploy/caddy/Caddyfile`)

Caddy es el ejemplo recomendado: renueva certificados solo y fuerza HTTPS por defecto.

```bash
caddy run --config deploy/caddy/Caddyfile --adapter caddyfile
```

## Nginx (`deploy/nginx/geostream.conf`)

Notas para Nginx, equivalente funcional al Caddyfile. Requiere que el operador gestione los
certificados (por ejemplo con `certbot`); recuerda que Nginx necesita `proxy_http_version 1.1`
y las cabeceras de Upgrade para que el stream funcione.

## Verificación

```bash
# TLS 1.2 o superior y HSTS presentes.
curl -sSI https://geostream.example.com/health/live | grep -i strict-transport-security
openssl s_client -connect geostream.example.com:443 -tls1_2 </dev/null 2>/dev/null | grep "Protocol"

# El upgrade de WebSocket responde 101 (con un ticket válido, docs/06 §5).
curl -sS -o /dev/null -w '%{http_code}\n' \
  -H 'Upgrade: websocket' -H 'Connection: Upgrade' \
  -H 'Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==' \
  -H 'Sec-WebSocket-Version: 13' \
  https://geostream.example.com/v1/stream
```

La comprobación forma parte del checklist de liberación (`docs/templates/RELEASE_TEMPLATE.md` §3).
