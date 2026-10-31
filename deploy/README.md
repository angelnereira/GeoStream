# GeoStream — despliegue

## Imagen única `all-in-one` (despliegue, portabilidad y servicio)

`deploy/Dockerfile` construye **una sola imagen** que contiene PostgreSQL, Redis y el
binario `geostream`. Es la forma de desplegar, portar y servir GeoStream en cualquier
host con Docker, sin orquestador ni servicios externos:

```bash
# Construir (desde la raíz del repo).
docker build -f deploy/Dockerfile -t geostream:all-in-one .

# Ejecutar. Un solo contenedor expone la API/stream en 8080.
docker run -d --name geostream -p 8080:8080 geostream:all-in-one serve
```

En el arranque el entrypoint:

1. inicializa el clúster PostgreSQL en `/data/pgdata` (volumen persistente),
2. crea el usuario y la base `geostream_dev`,
3. arranca Redis en `127.0.0.1:6379`,
4. genera el par de firma Ed25519 dev en `/data/jwt-keys` (montar un volumen en producción),
5. ejecuta migraciones (`GS_AUTO_MIGRATE=true`) y arranca `geostream serve`.

### Configuración por entorno

Todo se configura con variables `GS_*` (ver `docs/17_CONFIGURATION.md`). Las más útiles:

| Variable | Por defecto en la imagen | Qué hace |
|---|---|---|
| `GS_HTTP_ADDR` | `0.0.0.0:8080` | Listener HTTP (API + stream) |
| `GS_DATABASE_URL` | `postgres://geostream:…@127.0.0.1:5432/geostream_dev` | PostgreSQL |
| `GS_REDIS_URL` | `redis://127.0.0.1:6379` | Almacén efímero / bus |
| `GS_JWT_KEYS_DIR` | `/data/jwt-keys` | Directorio de pares `<kid>.key.pem` / `<kid>.pub.pem` |
| `GS_JWT_ACTIVE_KID` | `dev-key-1` | `kid` de firma activo |
| `GS_AUTO_MIGRATE` | `true` | Migrar al arrancar (dev) |

En producción: monta un volumen persistente en `/data` (`-v geostream-data:/data`),
sustituye las claves dev por las reales y **no** expongas la imagen directamente a
Internet: termínala con el proxy TLS de abajo.

### Persistencia

```bash
docker run -d --name geostream -p 8080:8080 \
  -v geostream-data:/data \
  geostream:all-in-one serve
```

### Verificación automática

```bash
deploy/smoke.sh            # construye, levanta y comprueba /health/ready
```

### Multi-arquitectura

El `Dockerfile` es multi-stage y no depende de la arquitectura del host en su lógica;
para publicar amd64+arm64:

```bash
docker buildx build --platform linux/amd64,linux/arm64 \
  -f deploy/Dockerfile -t <registry>/geostream:latest --push .
```

## Proxy TLS de referencia (NFR-SEC-001, docs/16 §2.3)

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

## Railway (producción gestionada)

Proyecto propio en Railway con tres servicios:

| Servicio | Origen | Notas |
|---|---|---|
| `Postgres` | plantilla gestionada de Railway | `GS_DATABASE_URL=${{Postgres.DATABASE_URL}}` |
| `Redis` | plantilla gestionada de Railway | `GS_REDIS_URL=${{Redis.REDIS_URL}}`, `GS_STATE_BACKEND=redis` |
| `geostream` | este repositorio, `deploy/Dockerfile.railway` (ajuste del servicio) | volumen en `/data`; healthcheck `/health/ready`; redespliega solo si cambian `apps/`, `crates/`, `contracts/`, `migrations/` o sus archivos de despliegue |

`deploy/railway-entrypoint.sh` genera en el volumen, la primera vez, el par Ed25519 (`/data/jwt-keys`) y la clave
`GS_DATA_ENCRYPTION_KEY` (`/data/secrets`), sin imprimirlos, y corre `geostream migrate` antes de `serve`
(`GS_AUTO_MIGRATE` está prohibido en `prod`). Variables del servicio: `GS_ENV=prod`,
`GS_PUBLIC_BASE_URL=https://<dominio público>`, `GS_DATABASE_URL`, `GS_REDIS_URL`, `GS_STATE_BACKEND=redis`.

Alta del primer tenant, app y API key: `geostream admin …` (`docs/16` §3) desde una shell del servicio
(`railway ssh`). La API key se muestra una sola vez: guárdala directamente en el gestor de secretos del integrador,
nunca en chats, tickets ni en el repositorio.

### Motor de rutas `osrm`
Servicio aparte en el mismo proyecto (`deploy/osrm/Dockerfile`, ajustado en el servicio; redespliega solo si cambia `deploy/osrm/`): OSRM 5.27
con el extracto de Panamá de Geofabrik, preprocesado al construir (perfil `car`, algoritmo MLD). Lo usa GravitalPay
para dibujar la ruta del repartidor al cliente (`GET /route/v1/driving/{lon},{lat};{lon},{lat}?overview=full&geometries=polyline6`).
Datos ODbL: la app muestra «© OpenStreetMap contributors». Para refrescar el mapa, redesplegar (vuelve a descargar el extracto).

La configuración de cada servicio (Dockerfile, healthcheck, reinicio, rutas vigiladas) vive en los ajustes del servicio en Railway, no en un `railway.toml`: Railway aplicaría un `railway.toml` de la raíz a todos los servicios del repositorio.
