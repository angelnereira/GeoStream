# 16 — Operación, despliegue y observabilidad

**ID:** GS-OPS-001 · **Versión:** 0.2.0 · **Requisitos:** FR-OPS-*, FR-ADM-001, NFR-OBS-*, NFR-RES-*, NFR-AVAIL-001

## 1. Topologías
Ver `04_ARCHITECTURE.md` §10 (T1 local, T2 nodo único, T3 multi-nodo, T4 roles separados). Dimensionamiento: se fija con los benchmarks de F6 (`docs/benchmarks/`); hasta entonces no se publican cifras de capacidad.

## 2. Dependencias y su configuración

### 2.1 PostgreSQL (≥ 16)
- Usuario de aplicación con permisos DML sobre el esquema; usuario de migraciones separado con DDL.
- `GS_DATABASE_MAX_CONNECTIONS` por instancia × instancias ≤ `max_connections` del servidor con margen (o PgBouncer en modo *transaction*, compatible porque no se usan sentencias preparadas con estado de sesión fuera de transacción — verificar con sqlx en F2).
- `statement_timeout` aplicado por la aplicación (`GS_DATABASE_STATEMENT_TIMEOUT_MS`).
- Backups: §9.

### 2.2 Redis o Valkey (≥ 7.2)
- **Sin persistencia**: `save ""` y `appendonly no` (las coordenadas no deben llegar a disco; NFR-PRIV-001).
- `maxmemory` dimensionado y `maxmemory-policy noeviction` (perder claves en silencio rompería garantías; mejor fallar visiblemente y alertar).
- Autenticación (`requirepass`/ACL) y red privada; TLS si cruza redes no controladas.
- Una sola instancia primaria es suficiente al inicio; réplicas/Sentinel/cluster solo con ADR (el bus usa Pub/Sub clásico; en cluster habría que evaluar Pub/Sub con *sharding*).
- Licencia: Redis cambió su licenciamiento en 2024–2025; Valkey es un fork con licencia BSD compatible en protocolo. Verificar la licencia vigente al elegir (ADR-0010).

### 2.3 Proxy / balanceador
- Soporte de WebSocket (HTTP/1.1 Upgrade). Timeout de inactividad del proxy > `GS_STREAM_IDLE_TIMEOUT_MS` + margen (recomendado ≥ 90 s).
- Sin afinidad de sesión requerida.
- Terminación TLS 1.2+; HSTS en público.
- Pasar `X-Forwarded-For` / `X-Forwarded-Proto`; el servidor solo los confía con `GS_TRUST_PROXY_HEADERS=true` y desde `GS_TRUSTED_PROXY_CIDRS`.
- Límite de tamaño de cuerpo ≥ `GS_REST_MAX_BODY_BYTES`.
- `deploy/` incluirá un ejemplo con Caddy (F6) y notas para Nginx.

## 3. CLI de administración

```text
geostream admin tenant create  --name <nombre>                       → imprime ten_…
geostream admin tenant list
geostream admin tenant disable --tenant <ten_…>
geostream admin tenant enable  --tenant <ten_…>
geostream admin app create     --tenant <ten_…> --name <nombre> [--device-scopes s1,s2] [--origins o1,o2]
geostream admin app list       --tenant <ten_…>
geostream admin app disable    --app <app_…>
geostream admin key create     --app <app_…> [--scopes s1,s2] [--label txt] [--expires-in 90d]   → imprime gsk_… UNA vez
geostream admin key list       --app <app_…>                         → sin secretos
geostream admin key revoke     --key <key_…>
geostream admin policy get     --tenant <ten_…>
geostream admin policy set     --tenant <ten_…> <clave>=<valor> …    → valida rangos (17 §4)
geostream admin signing-key generate --kid <kid> --out <dir>
geostream admin session close  --session <ses_…> --reason ADMIN      → para incidentes
```

Todas las acciones se auditan con `actor_kind=OPERATOR`. La CLI usa la misma configuración de base de datos que el servidor y publica en el bus los eventos de invalidación necesarios (`TenantDisabled`, `ApplicationDisabled`, `ApiKeyRevoked`) cuando hay almacén efímero configurado.

## 4. Observabilidad

### 4.1 Salud
- `/health/live`: 200 si el proceso atiende peticiones.
- `/health/ready`: 200 solo si PostgreSQL responde (consulta trivial con timeout 1 s), el almacén efímero responde (si es Redis/Valkey), el bus está conectado (si aplica) y la instancia no está drenando. Cuerpo `ReadinessStatus` con el estado por dependencia.

### 4.2 Métricas (Prometheus en `GS_ADMIN_ADDR/metrics`)

Labels permitidos: los listados. Prohibidos: cualquier ID de sesión, miembro, dispositivo, subject, tenant (NFR-OBS-002), coordenadas.

| Métrica | Tipo | Labels | Descripción |
|---|---|---|---|
| `gs_build_info` | gauge | `version`, `commit` | Siempre 1 |
| `gs_http_requests_total` | counter | `route`, `method`, `status_class` | `route` = plantilla (`/v1/sessions/{session_id}`) |
| `gs_http_request_duration_seconds` | histogram | `route`, `method` | |
| `gs_auth_failures_total` | counter | `principal_kind`, `reason` | `reason` ∈ códigos de error |
| `gs_rate_limited_total` | counter | `scope` | `rest_device`, `rest_app`, `invite_redeem`, `auth_ip`, `publish`, `stream_inbound`, `upgrade_ip` |
| `gs_stream_connections` | gauge | `principal_kind` | Conexiones abiertas |
| `gs_stream_connections_opened_total` | counter | `principal_kind` | |
| `gs_stream_connections_closed_total` | counter | `code` | Código de cierre |
| `gs_stream_upgrade_rejected_total` | counter | `reason` | 400/401/403/429/503 |
| `gs_stream_subscriptions` | gauge | `principal_kind` | |
| `gs_stream_messages_in_total` | counter | `type` | Nombre del `oneof` |
| `gs_stream_messages_out_total` | counter | `type` | |
| `gs_location_samples_total` | counter | `result` | `accepted`, `ignored_duplicate`, `ignored_stale`, `rejected` |
| `gs_location_rejected_total` | counter | `field` | Conjunto acotado de rutas de `09` §3.1 |
| `gs_stream_delivery_latency_seconds` | histogram | — | Aceptación → escritura en socket (NFR-PERF-001) |
| `gs_stream_coalesced_total` | counter | — | Ubicaciones reemplazadas antes de enviarse |
| `gs_stream_sequence_floor_applied_total` | counter | — | |
| `gs_fanout_deliveries_total` | counter | — | Entregas de `LocationUpdate` |
| `gs_revocation_propagation_seconds` | histogram | `kind` | Commit en PG → efecto en la conexión (NFR-SEC-002) |
| `gs_authz_cache_events_total` | counter | `event` | `hit`, `miss`, `revalidate`, `reload`, `flush` |
| `gs_bus_connected` | gauge | — | 1/0 |
| `gs_bus_messages_total` | counter | `direction`, `topic_kind` | `topic_kind` ∈ `loc`, `ctl`, `dev`, `global` |
| `gs_bus_reconnects_total` | counter | — | |
| `gs_ephemeral_command_duration_seconds` | histogram | `op` | |
| `gs_ephemeral_errors_total` | counter | `op` | |
| `gs_ephemeral_effect_failures_total` | counter | `effect` | Efectos post-commit fallidos (`11` §2.1) |
| `gs_db_query_duration_seconds` | histogram | `query` | Nombre lógico acotado |
| `gs_db_pool_connections` | gauge | `state` | `idle`, `in_use` |
| `gs_sessions_active` | gauge | — | Actualizado por el worker cada minuto |
| `gs_worker_job_duration_seconds` | histogram | `job` | |
| `gs_worker_job_rows_total` | counter | `job` | |
| `gs_webhook_deliveries_total` | counter | `result` | `delivered`, `retry`, `failed`, `disabled` |
| `gs_webhook_backlog` | gauge | — | Entregas pendientes vencidas |
| `gs_geofence_evaluations_total` | counter | — | F11 |
| `gs_geofence_evaluations_dropped_total` | counter | — | F11 |
| `gs_geofence_transitions_total` | counter | `transition` | F11 |

Más las métricas de proceso estándar del exportador.

### 4.3 Logs
JSON (`GS_LOG_FORMAT=json`) con campos: `ts`, `level`, `target`, `msg`, `request_id`, `connection_id`, `tenant_id`, `principal_kind`, `route`, `status`, `duration_ms`, `error_code`. Nunca los campos prohibidos de `10` §8.2. `tenant_id` sí puede aparecer en logs (no en labels).

### 4.4 Trazas
OpenTelemetry OTLP opcional (`GS_OTLP_ENDPOINT`). Spans por petición REST, por mensaje de stream relevante (subscribe, publish) y por job. Atributos sujetos a las mismas prohibiciones que los logs.

## 5. Alertas recomendadas

| Alerta | Condición (orientativa) |
|---|---|
| Instancia no lista | `/health/ready` ≠ 200 durante 2 min |
| Bus desconectado | `gs_bus_connected == 0` durante 1 min |
| Revocación lenta | p99 de `gs_revocation_propagation_seconds` > 2 s durante 10 min |
| Latencia de entrega | p99 de `gs_stream_delivery_latency_seconds` > 0,25 s durante 10 min |
| Tormenta de reconexión | tasa de `gs_stream_connections_opened_total` > 5× la media de 1 h |
| Consumidores lentos | aumento de cierres con código 4005 |
| Efectos efímeros fallidos | `gs_ephemeral_effect_failures_total` > 0 |
| Backlog de webhooks | `gs_webhook_backlog` creciente durante 15 min |
| Memoria de Redis/Valkey | > 80 % de `maxmemory` |
| Errores 5xx | > 1 % de peticiones durante 5 min |

## 6. Despliegue y apagado
- Orden en cada release: (1) `geostream migrate` (compatible hacia atrás, `11` §4); (2) despliegue gradual de instancias; (3) verificación de *ready* y métricas.
- Apagado: SIGTERM → drenaje (`08` §6.8) → salida en ≤ `GS_SHUTDOWN_GRACE_S` + 5 s (NFR-RES-003). El orquestador debe conceder al menos ese tiempo.
- Nunca desplegar automáticamente cambios incompatibles de protocolo o migraciones destructivas sin aprobación explícita (`19` §8).

## 7. Escalado
- `api`: por tasa de peticiones y latencia.
- `stream`: por conexiones, eventos/s, entregas/s, memoria y ancho de banda. El fan-out multiplica el egress: 1 publicación × N suscriptores = N entregas.
- `worker`: normalmente 1–2 instancias; los jobs son seguros con más.
- Particionar por tenant/región solo con evidencia (`04` §13).

## 8. Runbooks (se redactan completos en F12; resumen normativo)

| Runbook | Pasos clave |
|---|---|
| Redis/Valkey caído | Confirmar degradación explícita (NFR-RES-002); restaurar servicio; verificar `RECOVERED` y vaciado de caché; revisar `gs_ephemeral_effect_failures_total` |
| PostgreSQL caído | Instancias no listas; REST 503 (no hay mutaciones ni revocaciones posibles); las conexiones de stream existentes siguen funcionando con la caché mientras `mver` no cambie; las nuevas pueden fallar; restaurar; verificar migraciones |
| Revocación lenta | Revisar `gs_bus_connected`, latencia de Redis, `gs_authz_cache_events_total`; si el bus pierde mensajes, el peor caso sigue acotado por el TTL |
| Tormenta de reconexión | Verificar versión de SDK (backoff), capacidad de instancias, límites por IP; escalar `stream` |
| Rotación de clave de firma JWT | `06` §4.4 |
| API key comprometida | `geostream admin key revoke`; emitir una nueva; revisar auditoría |
| Rotación de `GS_DATA_ENCRYPTION_KEY` | Añadir nueva como activa y la anterior en `GS_DATA_ENCRYPTION_KEYS_OLD`; job de re-cifrado (F12); retirar la antigua |
| Incidente de privacidad | `10` §10 |
| Restaurar backup | §9 |
| Deshabilitar un tenant abusivo | `geostream admin tenant disable` (efecto ≤ NFR-SEC-002) |

## 9. Backups y recuperación
- PostgreSQL: backup completo diario + archivado continuo de WAL (recuperación a un punto en el tiempo). Objetivos iniciales: RPO ≤ 15 min, RTO ≤ 1 h (a validar).
- Prueba de restauración al menos trimestral y antes de GA (FR-OPS-006), documentada en `docs/ops/restore-drills/`.
- Redis/Valkey: sin backups (efímero por diseño). Tras su pérdida: los clientes reconectan, los snapshots se repueblan, las secuencias del SDK siguen creciendo.
- Tras restaurar PostgreSQL a un punto anterior: revocaciones posteriores al punto se pierden; el runbook exige re-ejecutar revocaciones críticas desde la auditoría del integrador.

## 10. Artefactos de despliegue (los crea el agente)
- `deploy/Dockerfile`: multi-etapa, binario estático o dinámico mínimo, usuario no-root, sin herramientas de compilación en la imagen final, `HEALTHCHECK` contra `/health/live`.
- `deploy/docker-compose.yml`: `geostream` (roles `api,stream,worker`), `postgres`, `valkey` (o `redis`) con la configuración de §2.2, volúmenes solo para PostgreSQL.
- `deploy/.env.example`: valores de desarrollo no secretos.
- `deploy/caddy/Caddyfile` (F6): TLS y proxy WebSocket de ejemplo.
