# 17 — Referencia de configuración y políticas

**ID:** GS-CFG-001 · **Versión:** 0.2.0 · **Requisitos:** FR-OPS-003, FR-ADM-002

Este es el **único** lugar donde viven los valores por defecto y rangos. Los demás documentos citan el nombre.

## 1. Reglas de carga
- Fuentes, de menor a mayor prioridad: valores por defecto → archivo TOML (`GS_CONFIG_FILE`, claves en minúsculas sin el prefijo `GS_`, p. ej. `stream_max_frame_bytes = 16384`) → variables de entorno `GS_*`.
- Toda clave se valida al arrancar (tipo y rango). Un valor inválido o desconocido detiene el arranque con un mensaje que nombra la clave (FR-OPS-003). En el archivo TOML, una clave desconocida también es error.
- Las claves marcadas **secreto** solo se aceptan por entorno o por archivo montado (`GS_<CLAVE>_FILE` con la ruta); nunca se registran.
- Restricciones por entorno: §3.
- Tipos: `dur_ms`/`dur_s` enteros; `list` separada por comas; `bool` = `true`/`false`.

## 2. Parámetros del servidor

### 2.1 General

| Clave | Tipo | Default | Rango / valores | Descripción |
|---|---|---|---|---|
| `GS_ENV` | enum | `dev` | `dev`, `test`, `prod` | Entorno; activa restricciones de §3 |
| `GS_CONFIG_FILE` | ruta | — | | Archivo TOML opcional |
| `GS_INSTANCE_ID` | string | UUID aleatorio al arrancar | `^[a-z0-9-]{1,64}$` | Identificador de instancia (presencia, logs) |
| `GS_HTTP_ADDR` | addr | `0.0.0.0:8080` | | Listener REST + stream + `/health` |
| `GS_ADMIN_ADDR` | addr | `127.0.0.1:9090` | | Listener de `/metrics` y `/health` |
| `GS_PUBLIC_BASE_URL` | url | `http://localhost:8080` | `https://` obligatorio en `prod` | Base pública; deriva `stream_url` (`wss://…/v1/stream`) y el issuer por defecto |
| `GS_TRUST_PROXY_HEADERS` | bool | `false` | | Confiar en `X-Forwarded-*` |
| `GS_TRUSTED_PROXY_CIDRS` | list | vacío | CIDR | Orígenes cuyos `X-Forwarded-*` se confían |
| `GS_SHUTDOWN_GRACE_S` | dur_s | 30 | 5..=300 | Tiempo de drenaje |
| `GS_LOG_FORMAT` | enum | `json` (`pretty` en `dev`) | `json`, `pretty` | |
| `GS_LOG_LEVEL` | string | `info` | filtro de `tracing` | |
| `GS_LOG_UNSAFE_LOCATIONS` | bool | `false` | solo `dev` | Permite registrar coordenadas para depuración local |
| `GS_OTLP_ENDPOINT` | url | — | | Exportación de trazas OTLP opcional |

### 2.2 PostgreSQL

| Clave | Tipo | Default | Rango | Descripción |
|---|---|---|---|---|
| `GS_DATABASE_URL` | url (**secreto**) | — (obligatorio) | | Cadena de conexión |
| `GS_DATABASE_MAX_CONNECTIONS` | int | 20 | 1..=200 | Tamaño del pool por instancia |
| `GS_DATABASE_CONNECT_TIMEOUT_MS` | dur_ms | 5000 | 100..=60000 | |
| `GS_DATABASE_STATEMENT_TIMEOUT_MS` | dur_ms | 5000 | 100..=60000 | `statement_timeout` por conexión |
| `GS_AUTO_MIGRATE` | bool | `false` | solo `dev` | Migrar al arrancar |

### 2.3 Almacén efímero y bus

| Clave | Tipo | Default | Rango | Descripción |
|---|---|---|---|---|
| `GS_STATE_BACKEND` | enum | `memory` | `memory`, `redis` | `memory` = una sola instancia |
| `GS_REDIS_URL` | url (**secreto**) | — | obligatorio si `redis` | Redis o Valkey |
| `GS_REDIS_KEY_PREFIX` | string | `gs` | `^[a-z0-9_-]{1,32}$` | Prefijo `{p}` de claves y temas |
| `GS_REDIS_COMMAND_TIMEOUT_MS` | dur_ms | 500 | 50..=10000 | Timeout por comando; superarlo cuenta como fallo |
| `GS_PRESENCE_TTL_S` | dur_s | 90 | 30..=600 | TTL de presencia; renovación cada TTL/3 |
| `GS_AUTHZ_CACHE_TTL_MS` | dur_ms | 5000 | 500..=30000 | Antigüedad máxima de una decisión de autorización en caché (ADR-0009) |

### 2.4 Autenticación y cifrado

| Clave | Tipo | Default | Rango | Descripción |
|---|---|---|---|---|
| `GS_JWT_ISSUER` | string | `GS_PUBLIC_BASE_URL` | | Claim `iss` |
| `GS_JWT_KEYS_DIR` | ruta | — (obligatorio) | | Pares `<kid>.key.pem` / `<kid>.pub.pem` |
| `GS_JWT_ACTIVE_KID` | string | — (obligatorio) | `^[a-z0-9-]{1,64}$` | Clave de firma activa |
| `GS_JWT_LEEWAY_S` | dur_s | 30 | 0..=120 | Margen para `nbf`/`exp` |
| `GS_TOKEN_MIN_TTL_S` | dur_s | 300 | 60..=3600 | TTL mínimo pedible |
| `GS_TOKEN_DEFAULT_TTL_S` | dur_s | 900 | min..=max | TTL por defecto |
| `GS_TOKEN_MAX_TTL_S` | dur_s | 3600 | min..=86400 | TTL máximo |
| `GS_TICKET_TTL_S` | dur_s | 30 | 5..=120 | Vida de un ticket de stream |
| `GS_DATA_ENCRYPTION_KEY` | base64 (**secreto**) | — (obligatorio desde F3) | 32 bytes | Clave AES-256-GCM activa (`10` §5.6); su `key_id` = primeros 8 hex de SHA-256 de la clave |
| `GS_DATA_ENCRYPTION_KEYS_OLD` | list base64 (**secreto**) | vacío | | Claves anteriores solo para descifrar |

### 2.5 REST

| Clave | Tipo | Default | Rango | Descripción |
|---|---|---|---|---|
| `GS_REST_MAX_BODY_BYTES` | int | 65536 | 1024..=1048576 | Tamaño máximo de cuerpo |
| `GS_REST_RATE_LIMIT_DEVICE_RPM` | int | 120 | 1..=10000 | Por dispositivo |
| `GS_REST_RATE_LIMIT_APP_RPM` | int | 6000 | 1..=1000000 | Por API key |
| `GS_INVITE_REDEEM_RPM` | int | 10 | 1..=100 | Canjes por dispositivo |
| `GS_AUTH_FAILURE_RPM_PER_IP` | int | 60 | 1..=10000 | Fallos de autenticación por IP |

### 2.6 Stream

| Clave | Tipo | Default | Rango | Descripción |
|---|---|---|---|---|
| `GS_STREAM_JSON_ENABLED` | bool | `true` (en `prod`: `false`) | | Habilita `gs.v1.json` |
| `GS_STREAM_MAX_CONNECTIONS` | int | 20000 | 1..=1000000 | Por instancia |
| `GS_STREAM_MAX_FRAME_BYTES` | int | 16384 | 1024..=1048576 | Trama entrante máxima |
| `GS_STREAM_HELLO_TIMEOUT_MS` | dur_ms | 5000 | 1000..=30000 | |
| `GS_STREAM_HEARTBEAT_INTERVAL_MS` | dur_ms | 20000 | 5000..=60000 | Informado en `limits` |
| `GS_STREAM_IDLE_TIMEOUT_MS` | dur_ms | 60000 | ≥ 2 × heartbeat, ≤ 300000 | Informado en `limits` |
| `GS_STREAM_MAX_SUBSCRIPTIONS` | int | 20 | 1..=200 | Por conexión de dispositivo |
| `GS_STREAM_MAX_SUBSCRIPTIONS_APP` | int | 1000 | 1..=100000 | Por conexión de aplicación |
| `GS_STREAM_OUTBOUND_CONTROL_QUEUE` | int | 1024 | 64..=65536 | Cola de control por conexión |
| `GS_STREAM_PENDING_EVENTS_MAX` | int | 256 | 16..=4096 | Eventos retenidos durante una suscripción |
| `GS_STREAM_OFFLINE_GRACE_MS` | dur_ms | 10000 | 0..=120000 | Gracia antes de emitir `OFFLINE` |
| `GS_STREAM_AUTH_EXPIRING_LEAD_S` | dur_s | 60 | 10..=600 | Antelación de `AuthExpiring` |
| `GS_STREAM_APP_MAX_CONNECTION_AGE_S` | dur_s | 3600 | 300..=86400 | Vida máxima de una conexión de aplicación |
| `GS_STREAM_UPGRADE_RPM_PER_IP` | int | 120 | 1..=100000 | Upgrades por IP |
| `GS_STREAM_MAX_INBOUND_MSGS_PER_S` | int | 20 | 1..=1000 | Mensajes entrantes por conexión |
| `GS_STREAM_ABUSE_WINDOW_S` | dur_s | 10 | 1..=300 | Exceso sostenido durante esta ventana → cierre 4429 |
| `GS_STREAM_MAX_PROTOCOL_ERRORS` | int | 10 | 1..=100 | Errores de protocolo antes de cerrar con 4000 |
| `GS_STREAM_DRAIN_SPREAD_MS` | dur_ms | 5000 | 0..=60000 | Dispersión de `reconnect_after_ms` al drenar |

### 2.7 Ubicación

| Clave | Tipo | Default | Rango | Descripción |
|---|---|---|---|---|
| `GS_LOCATION_MAX_FUTURE_SKEW_MS` | dur_ms | 5000 | 0..=60000 | Tolerancia de `captured_at` en el futuro |
| `GS_LOCATION_MAX_EVENT_AGE_MS` | dur_ms | 120000 | 5000..=86400000 | Antigüedad máxima aceptada (informada en `limits.max_event_age_ms`) |

### 2.8 Webhooks

| Clave | Tipo | Default | Rango | Descripción |
|---|---|---|---|---|
| `GS_WEBHOOK_TIMEOUT_MS` | dur_ms | 10000 | 1000..=30000 | Timeout por entrega |
| `GS_WEBHOOK_MAX_ATTEMPTS` | int | 10 | 1..=20 | Intentos totales |
| `GS_WEBHOOK_DISABLE_AFTER_H` | dur_h | 72 | 1..=720 | Fallos ininterrumpidos antes de deshabilitar |
| `GS_WEBHOOK_ALLOW_PRIVATE_NETWORKS` | bool | `false` | | Permitir destinos privados (pruebas) |
| `GS_WEBHOOK_ALLOWED_PORTS` | list int | `443` | 1..=65535 | Puertos de destino permitidos (en `dev` cualquiera) |

### 2.9 Worker y retención

| Clave | Tipo | Default | Rango | Descripción |
|---|---|---|---|---|
| `GS_WORKER_EXPIRY_INTERVAL_S` | dur_s | 15 | 1..=300 | Barrido de expiración |
| `GS_WORKER_WEBHOOK_POLL_MS` | dur_ms | 1000 | 100..=60000 | Sondeo del outbox |
| `GS_WORKER_RETENTION_HOUR_UTC` | int | 3 | 0..=23 | Hora del job de retención |
| `GS_WORKER_BATCH_SIZE` | int | 500 | 10..=10000 | Filas por lote |
| `GS_RETENTION_AUDIT_D` | dur_d | 365 | 1..=3650 | |
| `GS_RETENTION_IDEMPOTENCY_H` | dur_h | 24 | 1..=168 | |
| `GS_RETENTION_INVITATIONS_D` | dur_d | 30 | 1..=365 | Tras expirar/revocar |
| `GS_RETENTION_WEBHOOK_DELIVERIES_D` | dur_d | 30 | 1..=365 | Tras finalizar |
| `GS_RETENTION_CLOSED_SESSIONS_D` | dur_d | 90 | 1..=3650 | Sesiones cerradas/expiradas (cascada a membresías e invitaciones) |

### 2.10 Motor: trayectoria, pools e historial

> Todos los valores de esta sección son **valores iniciales** migrating desde `docs/proposals/GeoStream_Addendum_A02_Motor_Ubicacion_Movilidad.md` §14.5. Se calibran con medición (P-6) y nunca se afirman sin evidencia.

| Clave | Tipo | Default | Rango | Descripción |
|---|---|---|---|---|
| `GS_TRACK_CHECKPOINT_INTERVAL_S` | dur_s | 30 | 5..=300 | Periodicidad del punto de control del filtro (`22` §2.1) |
| `GS_TRACK_STATE_TTL_S` | dur_s | 600 | 60..=86400 | TTL del estado del filtro en el almacén efímero |
| `GS_TRACK_PROCESS_NOISE_STATIONARY_MPS2` | float | 0.05 | 0.001..=10.0 | Ruido de proceso con `motion=STATIONARY` |
| `GS_TRACK_PROCESS_NOISE_WALKING_MPS2` | float | 0.5 | 0.001..=10.0 | Ídem con `motion=WALKING` |
| `GS_TRACK_PROCESS_NOISE_RUNNING_MPS2` | float | 1.0 | 0.001..=10.0 | Ídem con `motion=RUNNING` |
| `GS_TRACK_PROCESS_NOISE_CYCLING_MPS2` | float | 1.0 | 0.001..=10.0 | Ídem con `motion=CYCLING` |
| `GS_TRACK_PROCESS_NOISE_AUTOMOTIVE_MPS2` | float | 2.0 | 0.001..=10.0 | Ídem con `motion=AUTOMOTIVE` |
| `GS_TRACK_PROCESS_NOISE_UNKNOWN_MPS2` | float | 1.5 | 0.001..=10.0 | Ídem con `motion=UNKNOWN` |
| `GS_TRACK_ZUPT_SIGMA_MPS` | float | 0.1 | 0.01..=5.0 | σ de la pseudo-medición de velocidad cero (anti-deriva) |
| `GS_TRACK_SPEED_TABLE_MAX_MPS` | string | `WALKING:2.0,RUNNING:8.0,CYCLING:12.0,AUTOMOTIVE:70.0,UNKNOWN:70.0` | — | `v_max(motion)` para la puerta de plausibilidad |
| `GS_POOL_INDEX_QUEUE` | int | 65536 | 1024..=1048576 | Capacidad del canal de actualización del índice vivo |
| `GS_POOL_PRUNE_INTERVAL_S` | dur_s | 30 | 5..=600 | Poda de entradas caducadas del pool |
| `GS_HISTORY_QUEUE_CAPACITY` | int | 100000 | 1000..=10000000 | Capacidad del canal del escritor de historial |
| `GS_HISTORY_BATCH_SIZE` | int | 1000 | 10..=100000 | Filas por lote de escritura |
| `GS_HISTORY_FLUSH_MS` | dur_ms | 500 | 10..=60000 | Intervalo máximo de vaciado del lote |
| `GS_HISTORY_PARTITION_LOOKAHEAD_DAYS` | int | 7 | 1..=60 | Antelación de creación de particiones diarias |
| `GS_HISTORY_H3_RESOLUTION` | int | 9 | 5..=12 | Resolución H3 para particionado del historial |
| `GS_GRAPH_PATHS` | string | (vacío) | — | Rutas de grafos `.gsg` a cargar (separadas por `:`); vacío = sin grafo propio |
| `GS_MM_SIGMA_Z_M` | float | 4.0 | 0.1..=100.0 | σ de emisión del HMM de map-matching (a calibrar, T-E7-06) |
| `GS_MM_BETA_M` | float | 0.5 | 0.01..=10.0 | Parámetro de transición exponencial (a calibrar, T-E7-06) |
| `GS_MM_CANDIDATE_RADIUS_M` | float | 50.0 | 5.0..=500.0 | Radio de búsqueda de candidatos en la vía |
| `GS_MM_MAX_CANDIDATES` | int | 8 | 1..=64 | Máximo de candidatos por muestra |
| `GS_MM_MAX_ROUTE_FACTOR` | float | 1.4 | 1.0..=5.0 | Acotación del Dijkstra por ruta |
| `GS_MM_FIXED_LAG` | int | 5 | 1..=50 | Retardo fijo del map-matching online |
| `GS_MM_LIVE_MIN_CONFIDENCE` | float | 0.6 | 0.0..=1.0 | Confianza mínima para emitir `MAP_MATCHED` en vivo |
| `GS_LEARNED_SPEED_RESOLUTION` | int | 9 | 5..=12 | Resolución H3 de los perfiles de velocidad |
| `GS_LEARNED_PLACES_EPS_M` | float | 80.0 | 5.0..=1000.0 | ε de DBSCAN para lugares aprendidos |
| `GS_LEARNED_PLACES_MIN_POINTS` | int | 20 | 2..=1000 | `minPts` de DBSCAN |
| `GS_OD_RESOLUTION` | int | 8 | 4..=12 | Resolución H3 de las matrices origen-destino |
| `GS_PROVIDERS_ENABLED` | bool | `false` | | Activación global del Provider Gateway |
| `GS_PROVIDERS_TIMEOUT_MS` | dur_ms | 5000 | 100..=60000 | Timeout por llamada al proveedor |
| `GS_PROVIDERS_MAX_CONCURRENT` | int | 16 | 1..=256 | Solicitudes concurrentes por proveedor |
| `GS_PROVIDERS_MAX_CACHE_TTL_S` | dur_s | 3600 | 0..=86400 | TTL máximo de caché de respuestas |
| `GS_EDGE_FINDER_MAX_ACCURACY_M` | float | 50.0 | 1.0..=500.0 | Precisión máxima del buscador para aportar un reporte |
| `GS_EDGE_RATE_TTL_S` | dur_s | 60 | 10..=3600 | TTL del contador de sightings por (buscador, `hashed_eid`) |

### 2.11 Constantes del motor (no configurables; fijadas por diseño)

Estas constantes **no** se configuran por entorno ni por tenant: son del modelo
geodésico o del algoritmo. Viven en `gs-geo` (geodesia) y en el motor que las usa.

**Elipsoide y esfera son cosas distintas y tienen nombres distintos.** Confundirlas
es la fuente clásica de un error del 0,3 % del que nadie sabe dónde sale.

| Constante | Valor | Qué es | Dónde |
|---|---|---|---|
| `EARTH_MEAN_RADIUS_M` | 6 371 008.8 | Radio medio de la esfera equivalente (IUGG, Moritz 2000). Es el que usa la fórmula de haversine. | `gs-geo` |
| `EARTH_SEMIMAJOR_AXIS_M` | 6 378 137.0 | Semieje mayor del elipsoide WGS84. | `gs-geo` |
| `EARTH_FLATTENING` | 1 / 298,257 223 563 | Achatamiento del WGS84. | `gs-geo` |

Las distancias **geodésicas** (Vincenty sobre el elipsoide) y las **esféricas**
(haversine) difieren en hasta un 0,5 % según la latitud. `gs-geo` expone las dos y
**dice cuál usó** (`DistanceMethod`), en vez de devolver un número sin origen. El
odómetro y las paradas usan la esférica, que es más barata y suficiente a escala
local; la calidad de trayectoria y el map-matching usan la geodésica.

| Constante | Valor | Dónde |
|---|---|---|
| Velocidad máxima por defecto de plausibilidad (sin `motion`) | 70.0 m/s | `22` §2.2 |
| Tabla de penalizaciones de plausibilidad | velocidad 0,8 · outlier 0,6 · mock 0,5 · fix congelado 0,5 · aceleración 0,4 · baja precisión 0,2 · solo red 0,1 | `22` §2.2 |
| Incertidumbre inicial de velocidad (`σ_v0`) | 10.0 m/s | `22` §2.3 |
| Velocidades por modo del estimador de ETA | auto 6,9 m/s · moto 8,3 m/s · bicicleta 4,2 m/s · a pie 1,3 m/s | `23` §4 |
| Tolerancia de simplificación online | `offline_simplify_tolerance_m`, en **metros**, no en grados | `22` §3.5 |

## 3. Restricciones por entorno

| Regla | `dev` | `test` | `prod` |
|---|---|---|---|
| `GS_PUBLIC_BASE_URL` con `https://` | no | no | **sí** |
| `GS_LOG_UNSAFE_LOCATIONS=true` | permitido | error | error |
| `GS_AUTO_MIGRATE=true` | permitido | permitido | error |
| `ws://` aceptado | sí | sí | no |
| Webhooks `http://` | sí | sí | no |
| `GS_STATE_BACKEND=memory` | sí | sí | permitido con advertencia en log al arrancar: solo una instancia |
| Secretos con valores de ejemplo de `deploy/.env.example` | permitido | permitido | error |

## 4. Política por tenant

Se guarda en `tenants.policy` (JSONB) y se gestiona con `geostream admin policy`. Las claves ausentes usan el default. Las claves desconocidas se rechazan. La política efectiva se cachea por instancia ≤ `GS_AUTHZ_CACHE_TTL_MS`.

| Clave | Tipo | Default | Rango | Usado en |
|---|---|---|---|---|
| `default_session_ttl_seconds` | int | 86400 | 60..=`max_session_ttl_seconds` | `09` §5 |
| `max_session_ttl_seconds` | int | 604800 | 3600..=31536000 | `05` §4.1 |
| `default_max_members_one_to_many` | int | 100 | 2..=`max_members_cap` | `05` §4.2 |
| `default_max_members_many_to_many` | int | 50 | 2..=`max_members_cap` | `05` §4.2 |
| `max_members_cap` | int | 1000 | 2..=10000 | `05` §4.1 |
| `snapshot_ttl_default_seconds` | int | 600 | 0..=`snapshot_ttl_max_seconds` | `09` §5 |
| `snapshot_ttl_max_seconds` | int | 86400 | 0..=86400 | `05` §4.1 |
| `stale_after_seconds_default` | int | 60 | 5..=3600 | `09` §5 |
| `min_publish_interval_ms` | int | 1000 | 200..=60000 | `08` §7.4, `limits` |
| `publish_burst` | int | 5 | 1..=20 | `08` §7.4 |
| `max_samples_per_publish` | int | 1 | 1..=50 | `08` §6.4, `limits` |
| `max_samples_per_publish_http` | int | 50 | 1..=50 | `07` §9.8 |
| `max_horizontal_accuracy_m` | float | 50000 | 5..=100000 (DR-20261031-03) | `09` §3.1 |
| `reject_null_island` | bool | `true` | | `09` §3.1 |
| `reject_mock_samples` | bool | `false` | | `09` §3.1 regla 17 (DR-20261031-02) |
| `reject_network_only` | bool | `false` | | `09` §3.1 regla 18 (DR-20261031-02) |
| `max_active_sessions_per_subject` | int | 50 | 1..=1000 | `09` §5 |
| `max_devices_per_subject` | int | 10 | 1..=100 | `05` §3.4 |
| `invitation_max_ttl_seconds` | int | 604800 | 60..=2592000 | `05` §6 |
| `invitation_max_uses` | int | 100 | 1..=10000 | `05` §6 |
| `max_webhooks_per_tenant` | int | 10 | 1..=100 | `07` §10 |
| `max_geofences_per_tenant` | int | 10000 | 0..=100000 | `14` §3 |
| `max_geofences_per_session` | int | 100 | 0..=1000 | `14` §3 |
| `geofence_confirm_samples` | int | 2 | 1..=10 | `14` §4 |
| `geofence_exit_buffer_m` | float | 20 | 0..=500 | `14` §4 |
| `geofence_max_accuracy_m` | float | 100 | 10..=1000 | `14` §4 |
| `history_allowed` | bool | `false` | `24` §1 | `03` FR-HIS-001 |

### 4.1 Política del motor (pista E)

> Valores iniciales migrados del addendum A-02 §14.5. Se calibran con medición y viven aquí en exclusiva (`00` §1, corolario 4).

**Trayectoria (`22` §2)**

| Clave | Tipo | Default | Rango | Usado en |
|---|---|---|---|---|
| `track_estimates_enabled` | bool | `true` | | `22` §2.7 |
| `track_max_speed_mps` | float | 70 | 10..=350 | `22` §2.2 |
| `track_max_accel_mps2` | float | 8 | 1..=50 | `22` §2.2 |
| `track_low_accuracy_m` | float | 100 | 10..=1000 | `22` §2.2 |
| `track_frozen_fix_count` | int | 5 | 2..=50 | `22` §2.2 |
| `track_reset_gap_s` | int | 60 | 5..=600 | `22` §2.3 |
| `track_lost_after_s` | int | 30 | 5..=600 | `22` §2.7 |
| `track_accuracy_sigma_factor` | float | 1.0 | 0.5..=3.0 | `22` §2.3 |
| `track_outlier_chi2` | float | 13.82 | 5..=50 | `22` §2.3 |
| `track_outlier_reset_count` | int | 3 | 1..=10 | `22` §2.3 |
| `track_frame_reanchor_m` | float | 10000 | 1000..=50000 | `22` §2.3 |
| `track_odometer_noise_factor` | float | 1.0 | 0.5..=5 | `22` §2.5 |
| `track_odometer_bridge_gaps` | bool | `true` | | `22` §2.5 |
| `track_stop_radius_m` | float | 50 | 10..=500 | `22` §2.6 |
| `track_stop_min_duration_s` | int | 120 | 10..=3600 | `22` §2.6 |
| `track_stop_exit_confirm` | int | 2 | 1..=10 | `22` §2.6 |

**Muestreo y publicación (`22` §3)**

| Clave | Tipo | Default | Rango | Usado en |
|---|---|---|---|---|
| `sampling_policy` | objeto | matriz de `22` §3.2 | validado por `22` §3.2 | `22` §3.4 |
| `publish_min_distance_m` | float | 10 | 1..=1000 | `22` §3.3 |
| `publish_accuracy_factor` | float | 1.5 | 0.1..=10.0 | `22` §3.3 |
| `publish_min_course_change_deg` | float | 20 | 1..=180 | `22` §3.3 |
| `publish_turn_min_speed_mps` | float | 2.5 | 0..=50 | `22` §3.3 |
| `publish_min_speed_change_mps` | float | 1.0 | 0..=20 | `22` §3.3 |
| `publish_max_silence_s` | int | 60 | 5..=600 | `22` §3.3 |

**Pools y movilidad (`23`)**

| Clave | Tipo | Default | Rango | Usado en |
|---|---|---|---|---|
| `max_pool_members_cap` | int | 100000 | 2..=1000000 | `23` §1.1 |
| `pool_snapshot_ttl_default_seconds` | int | 120 | 0..=`snapshot_ttl_max_seconds` | `23` §1.1 |
| `pool_index_resolution` | int | 9 | 7..=11 | `23` §2.1 |
| `pool_max_attributes` | int | 8 | 0..=16 | `23` §1.1 |
| `pool_query_max_radius_m` | float | 20000 | 100..=100000 | `23` §3.1 |
| `pool_eta_prefilter` | int | 20 | 1..=100 | `23` §3.1 |
| `pool_density_resolution` | int | 9 | 7..=10 | `23` §3.3 |
| `pool_density_max_markers` | int | 15 | 0..=50 | `23` §3.3 |
| `pool_density_delay_s` | int | 10 | 0..=120 | `23` §3.3 |
| `eta_default_circuity` | float | 1.35 | 1.0..=3.0 | `23` §4 |
| `eta_max_matrix_cells` | int | 625 | 1..=2500 | `23` §4 |
| `assignment_max_size` | int | 500 | 1..=2000 | `23` §5 |
| `max_dynamic_geofences_per_session` | int | 20 | 0..=200 | `23` §6 |
| `geofence_dwell_min_seconds` | int | 300 | 30..=86400 | `23` §6, `14` §5 |

**Historial y analítica (`24`)**

| Clave | Tipo | Default | Rango | Usado en |
|---|---|---|---|---|
| `history_raw_retention_days` | int | 30 | 1..=3650 | `24` §5 |
| `history_trip_retention_days` | int | 365 | 1..=3650 | `24` §5 |
| `history_backfill_max_age_s` | int | 604800 | 0..=2592000 | `24` §2 |
| `history_backfill_grace_s` | int | 86400 | 0..=604800 | `24` §2 |
| `analytics_allowed` | bool | `false` | | `24` §1 |
| `analytics_k_anonymity` | int | 5 | 2..=100 | `24` §6 |
| `trip_simplify_tolerance_m` | float | 5 | 0..=100 | `24` §3 |
| `driving_harsh_accel_mps2` | float | 3.0 | 1..=10 | `24` §3.2 |
| `driving_harsh_brake_mps2` | float | 3.5 | 1..=10 | `24` §3.2 |

**Edge (`25`)**

| Clave | Tipo | Default | Rango | Usado en |
|---|---|---|---|---|
| `edge_enabled` | bool | `false` | | `25` §2 |
| `edge_relay_scope` | enum | `TENANT` | `TENANT`, `NETWORK` | `25` §2.1 |
| `edge_rotation_s` | int | 1024 | 256..=86400 | `25` §2.3 |
| `edge_report_ttl_hours` | int | 168 | 1..=720 | `25` §2.6 |
| `edge_lookup_max_ids` | int | 500 | 1..=5000 | `25` §2.7 |
| `edge_separated_after_h` | int | 72 | 1..=720 | `25` §3 |
| `dult_min_sightings` | int | 3 | 1..=100 | `25` §3 |
| `dult_min_distinct_locations` | int | 3 | 1..=100 | `25` §3 |
| `dult_min_distance_m` | float | 500 | 10..=100000 | `25` §3 |
| `dult_min_duration_min` | int | 60 | 1..=10080 | `25` §3 |

**Interruptores de capacidad por tenant (pista E).** Todos por defecto `false`: una capacidad nueva no se activa para ningún tenant hasta que el propietario lo decide explícitamente (`00` §1, corolario 3).

| Clave | Tipo | Default | Habilita | Requisito cruzado |
|---|---|---|---|---|
| `pools_allowed` | bool | `false` | Sesiones `POOL` y consultas `nearby`/`rings`/`density` (`23`) | — |
| `tools_allowed` | bool | `false` | `POST /v1/tools/*` (`23` §4–§7) | — |
| `edge_allowed` | bool | `false` | Tags, reportes Beacon y LPWAN (`25`) | — |
| `learned_places_allowed` | bool | `false` | Capa de mapa aprendida y `GET /v1/analytics/places` (`24` §3) | `analytics_allowed = true` |
| `geofence_dwell_enabled` | bool | `false` | Eventos `geofence.dwell` (`23` §6) | `geofence_dwell_min_seconds` definido |
| `history_allowed` (línea base) | bool | `false` | Ingesta y consulta de historial (`24`) | — |
| `analytics_allowed` | bool | `false` | Agregados k-anónimos (`24` §6) | `history_allowed = true` |

Validación cruzada: `default_session_ttl_seconds ≤ max_session_ttl_seconds`; `default_max_members_* ≤ max_members_cap`; `snapshot_ttl_default_seconds ≤ snapshot_ttl_max_seconds`; `default_max_members_pool ≤ max_members_cap`; `pool_snapshot_ttl_default_seconds ≤ snapshot_ttl_max_seconds`; `analytics_allowed = true` exige `history_allowed = true`; `learned_places_allowed = true` exige `analytics_allowed = true`; `pool_index_resolution ≥ pool_density_resolution`.

## 5. Constantes del SDK (no configurables en el servidor)

| Constante | Valor | Dónde |
|---|---|---|
| Base de backoff | 500 ms | `12` §8 |
| Backoff máximo | 30 s | `12` §8 |
| Reinicio del contador de backoff | 60 s conectado | `12` §8 |
| Espera tras cierre 4429 | ≥ 60 s | `08` §8 |
| Bloque de reserva de secuencia | 100 | `12` §9 |
| Muestras de desfase de reloj | 8 | `12` §10 |
| Agrupación de emisiones de `SessionView` | 100 ms | `12` §14 |
| Renovación de token | cuando queda < 20 % de vida | `12` §8 |
| `offline_buffer_max_samples` | 20000 | `22` §3.5 |
| `offline_buffer_max_age_s` | 86400 | `22` §3.5 |
| `offline_simplify_tolerance_m` | 3 | `22` §3.5 |
| `history_upload_interval_s` | 300 | `22` §3.5 |
| `predict_horizon_ms` | 3000 (máximo 10000) | `22` §3.6 |
| `predict_blend_ms` | 500 | `22` §3.6 |
| `predict_speed_decay_s` | 2 | `22` §3.6 |
| `predict_min_speed_mps` | 1 | `22` §3.6 |
| `predict_min_plausibility` | 0.5 | `22` §3.6 |
| `predict_uncertainty_growth_mps` | 2 | `22` §3.6 |
| `sampling_activity_confidence` | 75 | `22` §3.2 |
| `sampling_downgrade_confirm_s` | 120 | `22` §3.2 |
| `wake_region_radius_m` | 150 | `22` §3.7 |
| `edge_scan_duty_cycle` | a medir (E8) | `25` §6 |

**Regla de acotamiento (T-30).** El núcleo del cliente acota **todo** valor recibido en `SamplingPolicy` a los rangos de las políticas de este documento antes de aplicarlo, e ignora valores ausentes o malformados. Un valor fuera de rango se reporta como `sampling_policy_clamped_total` (contador, sin identificadores) y **nunca** se aplica tal cual.

## 6. Variables de los ejemplos de integración
`GS_API_KEY` y `GS_BASE_URL` aparecen en `15_INTEGRATION_GUIDE.md` y `examples/`: son variables del **backend del integrador**, no del servidor GeoStream.
