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

Con `GS_STATE_BACKEND=redis`, `serve` conecta el almacén efímero y el bus a `GS_REDIS_URL` **antes** de abrir el
listener y no arranca si no lo alcanza (T-F6-01): una instancia con estado privado rompería en silencio la
distribución entre instancias. `11` §3 exige además, en el servidor Redis/Valkey, sin persistencia (`save ""`,
`appendonly no`) y `maxmemory-policy noeviction`; la plantilla gestionada de Railway hay que revisarla contra eso.

### Dominio público

| Host | Destino | DNS (en el proveedor de `gravitalcloud.com`) |
|---|---|---|
| `geostream.gravitalcloud.com` | servicio `geostream`, puerto 8080 | `CNAME geostream → 7z7x5pyj.up.railway.app` y `TXT _railway-verify.geostream` con el valor que muestra Railway |
| `geostream-production-cf6b.up.railway.app` | el mismo servicio | gestionado por Railway |

`GS_PUBLIC_BASE_URL=https://geostream.gravitalcloud.com`. El servicio despliega desde la rama `main`. El dominio
raíz y `api.gravitalcloud.com` pertenecen a otro proyecto de Railway: no reutilizarlos.

Alta del primer tenant, app y API key: `geostream admin …` (`docs/16` §3) desde una shell del servicio
(`railway ssh`). La API key se muestra una sola vez: guárdala directamente en el gestor de secretos del integrador,
nunca en chats, tickets ni en el repositorio.

Para consumir el servicio, cada integrador recibe su propia aplicación y API key; sus apps móviles reciben **tokens
de dispositivo** que emite su backend con `POST /v1/tokens` (`docs/15_INTEGRATION_GUIDE.md`). La app de pruebas
`apps/flutter_app` documenta el flujo completo con un token emitido a mano.

### Motor de rutas `osrm`
Servicio aparte en el mismo proyecto (`deploy/osrm/Dockerfile`, ajustado en el servicio; redespliega solo si cambia `deploy/osrm/`): OSRM 5.27
con el extracto de Panamá de Geofabrik, preprocesado al construir (perfil `car`, algoritmo MLD). Lo usa GravitalPay
para dibujar la ruta del repartidor al cliente (`GET /route/v1/driving/{lon},{lat};{lon},{lat}?overview=full&geometries=polyline6`).
Datos ODbL: la app muestra «© OpenStreetMap contributors». Para refrescar el mapa, redesplegar (vuelve a descargar el extracto).

La configuración de cada servicio (Dockerfile, healthcheck, reinicio, rutas vigiladas) vive en los ajustes del servicio en Railway, no en un `railway.toml`: Railway aplicaría un `railway.toml` de la raíz a todos los servicios del repositorio.

## Mapa propio (servicio `tiles`)

Teselas vectoriales derivadas del extracto de OpenStreetMap de Panamá (Geofabrik) con el esquema OpenMapTiles,
servidas por Martin (ADR-0034 §6, FR-MAP-007). Sustituye a `tile.openstreetmap.org`, cuya política no admite uso
intensivo en producción (`13` §2).

| Archivo | Qué es |
|---|---|
| `deploy/tiles/Dockerfile` | Construye las teselas con Planetiler al construir la imagen y las sirve con Martin. Imágenes fijadas por digest |
| `deploy/tiles/style.json` | Estilo MapLibre propio. Rutas relativas al origen: `/tiles/panama/{z}/{x}/{y}` y `/tiles/font/{fontstack}/{range}` |

- Servicio nuevo en Railway, origen este repositorio, `Dockerfile` en `deploy/tiles/Dockerfile`, directorio raíz
  `deploy/tiles`, sin dominio público: el portal lo expone como `/tiles/` (T-F15-12). Rutas vigiladas: `deploy/tiles/**`.
- Sirve `/panama` (TileJSON), `/panama/{z}/{x}/{y}` y `/font/{fontstack}/{range}` (glifos generados de Noto Sans).
- **El cliente debe hacer absolutas las rutas del estilo con su origen** antes de dárselo a MapLibre: sus workers no
  resuelven URLs relativas (`/tiles/…` falla con «Failed to parse URL»).
- Atribución obligatoria y ya incluida en el estilo: «© OpenStreetMap contributors» (ODbL) y «© OpenMapTiles»
  (CC-BY 4.0). `geostream-tools tiles-style` falla si falta o si el estilo apunta a servidores comunitarios.
- Actualizar los datos = reconstruir la imagen (Geofabrik publica el extracto a diario). No hay actualización en
  caliente.

**Medido** (esta máquina, 12 CPU y 30 GB, red doméstica, 2026-11-01; no es una cifra de Railway):

| Qué | Valor |
|---|---|
| Descarga de datos (capa propia, cacheable) | 9 a 23 min según el momento: Natural Earth 434 MB, polígonos de agua 932 MB, lagos 80 MB, extracto 36 MB; los servidores de terceros llegaron a ~200 kB/s |
| Generación de las teselas (con la descarga ya en caché) | 2 min 30 s; Planetiler pide 49 MB de montón para la lectura de OSM con `sparsearray` y recibe 3,2 GB |
| `panama.mbtiles` | 58 MB; tesela máxima 320 kB (199 kB comprimida) |
| Imagen final | 963 MB, de los cuales 853 MB son la imagen base de Martin |

**Un error que costó una prueba:** `--nodemap-type=array` hace que Planetiler pida 112 GB de montón para Panamá y se
niegue a empezar («Insufficient resources for read phase»). Una ejecución anterior lo ocultaba porque llevaba
`--force`, que se salta esa comprobación. El Dockerfile usa el valor por defecto (`sparsearray`) y nunca `--force`.
**En Railway** (revisión del 2026-11-01): el despliegue de `tiles` pasó a `SUCCESS` unos 2 min 40 s después de crear el
servicio, descarga de datos incluida (red de centro de datos, mucho más rápida que la doméstica); el consumo de memoria
del constructor no está medido.
