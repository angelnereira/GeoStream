# 07 — API REST v1 y webhooks (semántica)

**ID:** GS-API-001 · **Versión:** 0.2.0
**Contrato máquina (fuente de verdad de rutas y esquemas):** `contracts/openapi/geostream.v1.yaml` (OpenAPI 3.1)

Este documento define la **semántica** que OpenAPI no expresa. Autenticación y autorización: `06_IDENTITY_AUTH.md`. Validación y errores: `09_VALIDATION_ERRORS.md`.

## 1. Convenciones

| Aspecto | Regla |
|---|---|
| Base | `/v1`. Salud en `/health/*` (sin versión). |
| Formato | JSON UTF-8. Peticiones con cuerpo: `Content-Type: application/json`. Errores: `application/problem+json`. |
| Nombres de campo | `snake_case`. |
| Enums | Cadenas en mayúsculas: nombre Protobuf sin el prefijo del enum (`SESSION_MODE_ONE_TO_ONE` → `ONE_TO_ONE`). |
| Tiempos | RFC 3339 UTC con `Z` y milisegundos en respuestas. |
| Enteros grandes | `sequence` como número JSON (≤ 2^53 − 1). |
| Nulos | Los campos opcionales ausentes en un recurso se devuelven como `null` cuando el esquema lo declara; nunca se omiten campos `required`. |
| Campos desconocidos en peticiones | 422 (`09` §2). |
| Caché | Toda respuesta bajo `/v1` lleva `Cache-Control: no-store`. |
| Correlación | Toda respuesta lleva `X-Request-Id: req_…`; el mismo ID aparece en logs y en `Problem.request_id`. |

## 2. Cabeceras

Petición: `Authorization: Bearer <credencial>`, `Content-Type`, `Idempotency-Key` (POST mutantes), `If-Match` (PATCH).
Respuesta: `X-Request-Id`, `ETag` (recursos versionados: sesión, miembro), `Retry-After` (429, 503), `Idempotent-Replayed: true` (respuesta repetida por idempotencia), `Deprecation` / `Sunset` (cuando aplique, §11).

## 3. Autenticación

Ver `06`. Resumen: API key de aplicación o token de dispositivo en `Authorization: Bearer`. El esquema de seguridad y los scopes de cada operación están en OpenAPI (`security`, `x-gs-auth`, `x-gs-capability`).

## 4. Códigos de estado usados

`200`, `201` (creación), `202` (encolado), `204` (sin cuerpo), `400`, `401`, `403`, `404`, `405`, `409`, `412`, `413`, `415`, `422`, `429`, `500`, `503`. El significado de cada `code` está en `09` §6.

## 5. Problem Details

```json
{
  "type": "https://geostream.dev/problems/session_full",
  "title": "Session is full",
  "status": 409,
  "code": "SESSION_FULL",
  "detail": "max_members reached",
  "request_id": "req_0192b6c0a1e47c3f9d2a7b5e8f1c0d3a",
  "errors": []
}
```

- `type` = `https://geostream.dev/problems/` + `code` en minúsculas. Es un identificador, no tiene que resolver.
- `title`: frase fija por `code` (tabla en `gs-api`, en inglés).
- `detail`: opcional, ≤ 200 caracteres, sin datos sensibles.
- `errors`: solo en `VALIDATION_FAILED`, lista de `{field, reason}` (`09` §2.1).

## 6. Idempotencia

Aplica a todo `POST` que crea o muta (marcado con el parámetro `Idempotency-Key` en OpenAPI). `PUT`, `PATCH` con `If-Match` y `DELETE` son idempotentes por semántica.

Algoritmo:
1. Sin cabecera → procesar normalmente.
2. Clave de almacenamiento: `(tenant_id, principal_id, idempotency_key)`; `principal_id` = `key_id` de la API key o `device_id`.
3. Huella: SHA-256 de `método + " " + ruta + "\n" + cuerpo canónico` (JSON con claves ordenadas y sin espacios).
4. Si no existe: insertar registro `IN_PROGRESS` (insert con conflicto controlado) → procesar → guardar estado HTTP y cuerpo de la respuesta → `COMPLETED`.
5. Si existe `COMPLETED` con la misma huella → devolver la respuesta guardada con `Idempotent-Replayed: true`.
6. Si existe con otra huella → 422 `IDEMPOTENCY_KEY_REUSED`.
7. Si existe `IN_PROGRESS` → 409 `CONFLICT` (el cliente reintenta tras una breve espera).
8. Las respuestas 5xx no se guardan (el registro se elimina para permitir reintento). Las 4xx sí se guardan.
9. Retención: `GS_RETENTION_IDEMPOTENCY_H`.

Las respuestas que contienen secretos que solo se muestran una vez (código de invitación, secreto de webhook) **sí** se guardan para la repetición, cifradas en reposo con `GS_DATA_ENCRYPTION_KEY` y purgadas al vencer la retención. Ver `10` §6.

## 7. Paginación

- Parámetros `limit` (1..=100, default 50) y `cursor`.
- Respuesta `{ "data": [...], "next_cursor": "<opaco>" | null }`.
- Orden estable: `created_at DESC, id DESC` salvo que la operación indique otro.
- El cursor codifica en base64url la posición (`created_at`, `id`) y los filtros; el servidor rechaza un cursor malformado o usado con filtros distintos (422).
- Los filtros de autorización se aplican siempre: un cursor manipulado no amplía lo visible.

## 8. Rate limits

| Principal | Límite | Parámetro |
|---|---|---|
| Dispositivo | peticiones/minuto por dispositivo | `GS_REST_RATE_LIMIT_DEVICE_RPM` |
| Aplicación | peticiones/minuto por API key | `GS_REST_RATE_LIMIT_APP_RPM` |
| Canje de invitaciones | intentos/minuto por dispositivo | `GS_INVITE_REDEEM_RPM` |
| Fallos de autenticación | por IP | `GS_AUTH_FAILURE_RPM_PER_IP` |

Ventana fija de 60 s en el almacén efímero (memoria en nodo único). Al exceder: 429 `RATE_LIMITED` con `Retry-After` = segundos hasta el fin de la ventana (≥ 1). Las respuestas pueden incluir `RateLimit-Limit` y `RateLimit-Remaining` (informativas).

## 9. Semántica por recurso

### 9.1 Tokens y tickets
- `POST /v1/tokens` — `06` §4. Idempotente de forma natural para subject/dispositivo; cada llamada emite un token nuevo.
- `POST /v1/stream/tickets` — `06` §5. Respuesta 201.

### 9.2 Sesiones
- `POST /v1/sessions`:
  - Dispositivo: el subject del token queda como `OWNER` con las cuatro capacidades; `owner_external_subject_id` e `initial_members` → 422 `not_allowed`.
  - Aplicación: sin `owner_external_subject_id`, la sesión es **propiedad de la aplicación** (`owner_member_id = null`). Con él, ese subject (creado si no existe) queda como `OWNER`. `initial_members` crea membresías (`joined_via = DIRECT_ADD`) con capacidades explícitas o por defecto del modo.
  - Todo en una transacción. Se aplica `max_active_sessions_per_subject` al propietario y a cada miembro inicial.
  - Respuesta 201 con `Session` y `ETag`.
- `GET /v1/sessions` — dispositivo: sesiones `ACTIVE` y no expiradas donde su subject es miembro activo (los filtros `status`, `external_subject_id` → 422 si los envía un dispositivo); aplicación: todas las del tenant, filtrables.
- `GET /v1/sessions/{id}` — 404 si el dispositivo no es miembro activo.
- `PATCH /v1/sessions/{id}` — solo `ACTIVE`; otra → 409 `SESSION_CLOSED` / `SESSION_EXPIRED`. Cambiar `snapshot_ttl_seconds` a 0 borra los snapshots existentes. Emite `SessionEvent{UPDATED}` y webhook `session.updated`.
- `POST /v1/sessions/{id}/close` — efectos de `05` §4.3; `closed_reason` = `CLOSED_BY_MEMBER` (dispositivo) o `CLOSED_BY_APPLICATION`. Sobre una sesión ya cerrada o expirada devuelve 200 con su estado.

### 9.3 Miembros
- `GET …/members` — miembros `ACTIVE`, orden `joined_at ASC, id ASC`.
- `POST …/members` — 201; subject ya activo → 409 `CONFLICT`; sesión llena → 409 `SESSION_FULL`; sesión no activa → 409 `SESSION_*`.
- `GET …/members/{member_id|me}` — `me` solo para dispositivos (aplicación + `me` → 404).
- `PATCH …/members/{id}` — cambios de capacidades (INV-MEM-1..5) y rol (solo aplicación; `role: OWNER` transfiere la propiedad: el nuevo propietario recibe las cuatro capacidades y el anterior pasa a `MEMBER` conservando las suyas).
- `DELETE …/members/{id}` — revocación (`05` §5.2). Revocar a un miembro ya `REVOKED` → 204; `LEFT` → 204 sin cambios.
- `POST …/leave` — 204; efectos de `05` §5.2 con `LEFT`; aplica `05` §5.4 si es propietario.
- `PUT …/members/{id}/sharing` — `05` §7. Respuesta 200 con el `Member` actualizado. Idempotente.

### 9.4 Invitaciones
- `POST …/invitations` — 201 con `code` (única vez). Grants por defecto: los del modo para quien se une (`05` §4.2), recortados a las capacidades del emisor.
- `GET …/invitations` — sin `code`. `status` calculado: `REVOKED` > `EXPIRED` > `EXHAUSTED` > `ACTIVE`.
- `DELETE …/invitations/{id}` — 204, idempotente.
- `POST /v1/invitations/redeem` — 201 `{session, member}`; reglas de `05` §6.

### 9.5 Subjects y dispositivos
- `GET /v1/subjects?external_subject_id=` — lista con 0 o 1 elementos.
- `POST /v1/subjects/{id}/revoke-tokens` — incrementa `token_epoch`; devuelve el `Subject`.
- `DELETE /v1/subjects/{id}` (F10) — `10` §7.
- `GET /v1/subjects/{id}/export` (F10) — sin coordenadas.
- `GET /v1/devices/{id}` — dispositivo: solo dispositivos de su subject.
- `DELETE /v1/devices/{id}` — dispositivo: solo el propio; efectos en `06` §7.

### 9.6 Snapshot
- `GET /v1/sessions/{id}/snapshot` — miembros que comparten con snapshot vigente; `age_ms = generated_at − captured_at`. Sesión no activa → 409. Almacén efímero caído → 503.

### 9.7 Geocercas (F11)
Ver `14_GEOFENCING.md`.

### 9.8 Ingesta HTTP (F8)
- `POST /v1/ingest/locations` — mismas precondiciones que `08` §6.4 (devueltas como Problem con su HTTP) y mismo algoritmo por muestra (`09` §4). Respuesta 200 con `results` alineados con `samples`.
- Límite de muestras: `max_samples_per_publish_http` (política). Rate limit: el mismo token bucket por (dispositivo, sesión) que el stream, compartido.

### 9.9 Pools de disponibilidad (E4)
Ver `23_MOBILITY.md`. Todas estas operaciones exigen política de tenant `pools_allowed` y son **aditivas**: no cambian el comportamiento de `/v1/sessions` ni de `/v1/sessions/{id}/members` para los modos existentes.

| Operación | Scope de aplicación | Scope de dispositivo | Notas |
|---|---|---|---|
| `POST /v1/pools` | `pools:manage` | ✗ | Crea una sesión con `mode = POOL`. No acepta `owner_external_subject_id` ni `initial_members`; los conductores entran solos. |
| `GET /v1/pools/{id}` | `sessions:read` | ✗ | Devuelve la `Session` con `mode = POOL`. |
| `GET /v1/pools/{id}/nearby` | `pools:query` | ✗ | §9.9.1 |
| `GET /v1/pools/{id}/rings` | `pools:query` | ✗ | §9.9.2 |
| `GET /v1/pools/{id}/density` | `pools:query` | `pools:density` | §9.9.3; **sin identificadores** |
| `PUT /v1/pools/{id}/members/{member_id}/availability` | `pools:manage` | `sharing:write` (solo `me`) | `AVAILABLE` / `BUSY` / `PAUSED` |
| `PUT /v1/pools/{id}/members/{member_id}/attributes` | `pools:manage` | ✗ | Solo aplicación; ≤ `pool_max_attributes` entradas (`09` §5.1) |
| `POST /v1/pools/{id}/reset-odometer` | `members:write` | ✗ | Reinicia el odómetro de un miembro del pool |

`POST /v1/pools` es un alias de sugar sintáctico de `POST /v1/sessions` con `mode = POOL` forzado: **no** duplica reglas. Su respuesta es la misma `Session` con `ETag`.

#### 9.9.1 `nearby`
`GET /v1/pools/{id}/nearby?lat=&lon=&radius_m=&limit=&rank=&max_age_s=&filter.attributes[k]=v`

Ordena por `distance` (predeterminado) o por `eta`. Con `rank = eta` se aplica el prefiltrado `pool_eta_prefilter` (los N candidatos más cercanos) antes de estimar el tiempo de llegada. Cada elemento incluye `member_id`, `distance_m`, `age_ms`, `availability`, `attributes` (si los pidió el filtro), `position` y, con `rank = eta`, `eta_s` y `mode`.

Nunca incluye a quien está `PAUSED` ni a quien no está conectado, salvo que `max_age_s` lo permita explícitamente. Límite duro: `pool_query_max_radius_m`.

#### 9.9.2 `rings`
`GET /v1/pools/{id}/rings?lat=&lon=&ring_step_m=&k=&per_ring_limit=`

Devuelve anillos concéntricos desde el centro. Exactamente uno de `ring_step_m` o `k` debe enviarse. `per_ring_limit` acota cada anillo (1..=100); el resto se omite y la respuesta indica `truncated: true` en el anillo afectado. Solo para aplicación.

#### 9.9.3 `density`
`GET /v1/pools/{id}/density?lat=&lon=&radius_m=&resolution=`

Devuelve **marcadores aproximados y anónimos**, sin `member_id`, sin `subject_id`, sin `device_id` y sin marcas de tiempo exactas (`FR-POOL-006`). Cada marcador lleva un conteo, un centroide de celda H3 desplazado de forma aleatoria pero acotada, rumbo cuantizado a 8 sectores y `delay_ms ≥ pool_density_delay_s`. Máximo `pool_density_max_markers` marcadores. Es la **única** consulta de pool accesible a un dispositivo, y existe precisamente para que un pasajero no necesite ver conductores individuales.

### 9.10 Herramientas de movilidad (E5)
Ver `23_MOBILITY.md` §4–§7. Todos exigen `tools:use` y `tenant.tools_allowed`.

| Operación | Descripción |
|---|---|
| `POST /v1/tools/eta` | Matriz de tiempos de llegada origen→destino. Requiere scope de aplicación **o** de dispositivo con `tools:use`. |
| `POST /v1/tools/assignments` | Asignación por lotes de tareas a trabajadores; `hungarian` o `min_cost_flow`. |
| `POST /v1/tools/routes` | Ruta sobre el grafo vial propio o por Provider Gateway (`GS_GRAPH_PATHS`). |

Los tres aceptan `source_preference ∈ {PROVIDER, LEARNED, ESTIMATED}`. Con `PROVIDER` y el proveedor caído se devuelve `503 PROVIDER_UNAVAILABLE` con `Retry-After`; **nunca** se degrada silenciosamente a la estimación propia, porque el integrador pidió explícitamente el proveedor.

`GET/PUT/DELETE /v1/providers/{port}` (scope `providers:manage`) configuran el Provider Gateway: `port ∈ {ROUTE_MATRIX, ROUTE, MAP_MATCHING, GEOCODING}`, URL `https://`, credencial BYOK **del tenant** cifrada en reposo. El cuerpo de la petición **nunca** devuelve la credencial; `PUT` con la misma credencial la rota y emite auditoría `provider.rotated` (`23` §8).

### 9.11 Historial y analítica (E6/E7)
Ver `24_HISTORY_ANALYTICS.md`. Todas exigen la política correspondiente del tenant; sin ella devuelven `403 FEATURE_DISABLED`.

| Operación | Scope | Notas |
|---|---|---|
| `POST /v1/sessions/{id}/history` | dispositivo `ingest:write` | Ingesta de muestras retroactivas (`24` §2). |
| `GET /v1/sessions/{id}/trace?from=&to=&simplify_m=&format=` | `history:read` | Polilínea de la sesión; `format ∈ {json, polyline, gsp1}`. |
| `GET /v1/sessions/{id}/trips`, `GET /v1/trips/{trip_id}` | `history:read` | Viajes derivados. |
| `GET /v1/analytics/heatmap` | `analytics:read` | Agregado k-anónimo. |
| `GET /v1/analytics/od-matrix` | `analytics:read` | Pares origen-destino k-anónimos. |
| `GET /v1/analytics/speed-profile` | `analytics:read` | Percentiles por celda y franja. |
| `GET /v1/analytics/mobility-profile` | `analytics:read` | Exige consentimiento `ANALYTICS` del sujeto. |
| `GET /v1/analytics/places` | `analytics:read` | Lugares aprendidos, k-anónimos. |
| `POST /v1/exports`, `GET /v1/exports/{id}` | `history:read` | Exportación asíncrona; la URL de descarga caduca (`24` §4). |

Toda respuesta analítica incluye `k_anonymity_applied` y `suppressed_cells` para que el integrador sepa qué se ocultó, sin revelar los valores suprimidos.

### 9.12 Dispositivos edge (E8)
Ver `25_EDGE_DEVICES.md`. Todas exigen `edge_enabled` en la política del tenant.

| Operación | Scope de aplicación | Scope de dispositivo | Notas |
|---|---|---|---|
| `POST /v1/tags`, `GET /v1/tags`, `GET /v1/tags/{id}`, `DELETE /v1/tags/{id}` | `tags:manage` | `tags:manage` | Registro y baja. El cuerpo **nunca** acepta ni devuelve la `EIK` (`FR-EDGE-001`). |
| `POST /v1/edge/reports` | ✗ | `edge:report` | Reporte cifrado de sightings. |
| `POST /v1/edge/reports/lookup` | ✗ | `edge:lookup` | Consulta de los propios tags; el servidor entrega los blobs **sin poder descifrarlos**. |
| `POST /v1/edge/lpwan/{integration_id}/webhook` | ✗ | firma HMAC de integración | Ingesta LPWAN (`25` §4). |

`POST /v1/edge/reports/lookup` responde `429 RATE_LIMITED` con el bucket por (dueño, `hashed_eid`), porque es una operación cara y susceptible de abuso de descubrimiento (`FR-EDGE-005`).

### 9.13 Consola de desarrolladores (F15)

Contrato: `contracts/openapi/geostream.v1.yaml`, etiqueta `Console` (15 operaciones bajo `/v1/console`). Decisión: ADR-0033 §3–§4 y ADR-0036. Autorización: `06` §8.6.

- **Registro e inicio de sesión son una sola ceremonia.** `POST /v1/console/auth/code` responde siempre `202 {"accepted": true}` con el mismo cuerpo exista o no la cuenta; `POST /v1/console/auth/verify` canjea el código, crea la cuenta si hace falta (con `accept_terms: true`) y devuelve el token. Todo fallo de verificación es `401 UNAUTHENTICATED`, sin distinguir la causa.
- **Sesión**: token opaco `gcs_…` en `Authorization: Bearer`; sin cookies. Cierra con `POST /v1/console/auth/logout`.
- **Organización = tenant; proyecto = aplicación.** Crear una organización (`201`) crea el tenant con la política por defecto y hace `OWNER` a la cuenta; el límite es `GS_CONSOLE_MAX_ORGS_PER_ACCOUNT` (429 al superarlo).
- **API keys**: `createConsoleApiKey` devuelve `secret` una sola vez (`Cache-Control: no-store`); la lista y el resto de operaciones solo muestran el `prefix`. No hay valor por defecto de `scopes`.
- **Idempotencia**: las operaciones que crean (`createConsoleOrganization`, `createConsoleProject`, `createConsoleApiKey`) aceptan `Idempotency-Key` (`§6`).
- Los errores usan `Problem Details` (`§5`) y el catálogo de `09` §6; no se añade ningún código.


## 10. Webhooks

### 10.1 Modelo
Endpoints registrados por tenant (`/v1/webhooks`). Cada evento de dominio que proyecta a webhook (`05` §10) se escribe en el outbox **en la misma transacción** que el cambio de estado, una fila por endpoint suscrito al tipo. El rol `worker` entrega.

### 10.2 Catálogo de tipos
`session.created`, `session.updated`, `session.closed`, `session.expired`, `member.joined`, `member.updated`, `member.left`, `member.revoked`, `sharing.started`, `sharing.stopped`, `geofence.entered` (F11), `geofence.exited` (F11), `webhook.test`. Esquemas en OpenAPI (`webhooks:`).

**Tipos añadidos por la pista E**, todos **sin coordenadas** (FR-WHK-005):

| Tipo | Cuándo | Qué lleva |
|---|---|---|
| `geofence.dwell` (E5) | Permanencia ≥ `geofence_dwell_min_seconds` | `geofence_id`, `member_id`, `entered_at`, `dwell_s` |
| `track.stop_started`, `track.stop_ended` (E2) | Entrada y salida de parada online | `member_id`, `stop_index`, `duration_s`; **sin** centroide |
| `trip.completed` (E6) | Cierre de viaje | `trip_id`, `started_at`, `ended_at`, `distance_m`, `stop_count`; **sin** polilínea |
| `pool.member_online`, `pool.member_offline` (E4) | Conexión y desconexión en un pool | `session_id`, `member_id` |
| `pool.member_availability_changed` (E4) | Cambio de `availability` | `member_id`, `from`, `to` |
| `edge.tag_registered`, `edge.tag_removed` (E8) | Alta y baja de tag | `tag_id`, `model`; **nunca** la `EIK` |

Cada tipo lleva en OpenAPI la extensión `x-gs-phase` con la fase que lo introduce (`E2`, `E4`…), de modo que un integrador pueda filtrar por fase sin consultar este documento.

Envoltorio:

```json
{
  "id": "evt_…",
  "type": "member.revoked",
  "created_at": "2026-10-11T21:04:05.123Z",
  "tenant_id": "ten_…",
  "data": { "member": { "…": "…" } }
}
```

### 10.3 Firma
- Secreto: `whsec_` + 32 bytes CSPRNG en base64url (43 caracteres). Se muestra una vez al crear. Se almacena cifrado (`GS_DATA_ENCRYPTION_KEY`) porque se necesita para firmar.
- Cabeceras de cada entrega:
  - `GS-Webhook-Id: evt_…`
  - `GS-Webhook-Timestamp: <segundos Unix>`
  - `GS-Webhook-Signature: v1=<hex>`
- `hex` = HMAC-SHA256 en hexadecimal minúscula, con clave = bytes UTF-8 del secreto completo (incluido `whsec_`) y mensaje = `"{id}.{timestamp}.{cuerpo_bruto}"`.
- Varias firmas (rotación futura) se separan por espacio: `v1=<a> v1=<b>`. El receptor acepta si alguna coincide.
- Verificación recomendada al receptor: comparar en tiempo constante; rechazar si `|ahora − timestamp| > 300 s`; deduplicar por `GS-Webhook-Id`.
- Vector de prueba: `contracts/test-vectors/webhook_signature.json`.

### 10.4 Entrega y reintentos
- `POST` con `Content-Type: application/json`, `User-Agent: GeoStream-Webhooks/1`, timeout `GS_WEBHOOK_TIMEOUT_MS`. No se siguen redirecciones.
- Éxito: cualquier 2xx. `410 Gone` → el endpoint pasa a `DISABLED` de inmediato.
- Cualquier otro resultado (3xx, 4xx, 5xx, timeout, error de red) → reintento.
- Calendario (desde el primer intento): +10 s, +30 s, +2 min, +10 min, +30 min, +1 h, +3 h, +6 h, +12 h, con jitter ±10 %; máximo `GS_WEBHOOK_MAX_ATTEMPTS` intentos. Agotados → entrega `FAILED`.
- Un endpoint con fallos ininterrumpidos durante `GS_WEBHOOK_DISABLE_AFTER_H` horas pasa a `DISABLED`.
- Semántica **at-least-once**; sin orden garantizado. El receptor ordena por `created_at` y por la `version` del recurso incluido.

### 10.5 Contenido
- Ningún webhook contiene coordenadas (FR-WHK-005). Quien necesite la ubicación usa el snapshot o el stream con `locations:read`.
- `data` contiene el recurso REST completo (`Session`, `Member`) en su estado tras el evento.
- **Regla derivada del historial (E6/E7/E8):** tampoco contiene una `LocationEstimate`, una polilínea `gsp1`, un centroide de parada, una celda H3 ni un perfil por sujeto. Un webhook describe **qué pasó**, no **dónde**; la ubicación se pide por el canal autorizado que corresponda.

## 11. Versionado y deprecación
- Cambios aditivos (campos opcionales nuevos en respuestas, endpoints nuevos, valores de enum nuevos documentados como posibles) no cambian `/v1`. Los clientes DEBEN ignorar campos desconocidos en respuestas y tratar valores de enum desconocidos de forma segura.
- Cambios incompatibles → `/v2`, conviviendo con `/v1` durante ≥ 6 meses.
- Endpoints en deprecación responden con `Deprecation: @<unix>` y `Sunset: <fecha HTTP>`.

**Compatibilidad de la pista E (regla dura).** Todas las operaciones E4–E8 son **aditivas bajo `/v1`** y llevan `x-gs-phase` en OpenAPI. Ninguna modifica un contrato existente de forma incompatible:

- `Session.mode` suma el valor `POOL`; los clientes que no lo conocen lo tratan como `UNRECOGNIZED_MODE` y no asumen nada.
- `Member` suma `availability`, `attributes`, `purposes` y `version`; todos opcionales.
- `LocationUpdate` suma el campo `estimate` y el mensaje `TrackEvent` (campo 28 de `ServerMessage.body`), sin renumerar ningún campo existente.
- `ClientMessage.body` suma `Subscribe.replay_trace` sin renumerar.
- Los errores nuevos (`FEATURE_DISABLED`, `CONSENT_REQUIRED`, `PROVIDER_UNAVAILABLE`) solo aparecen ante peticiones que exigen capacidades nuevas; un cliente que nunca las usa nunca los verá.

Un cliente escrito antes de la pista E debe seguir funcionando **sin cambios** contra un servidor que sí la soporta (`NFR-COMPAT-003`).

## 12. CORS (F8)
Solo para orígenes en `application.allowed_origins`. Métodos y cabeceras permitidos: los de esta API. `Access-Control-Allow-Credentials: false` (la autenticación va en cabecera, no en cookies).

## 13. Ejemplos (ILUSTRATIVO)

```bash
# Backend del integrador: emitir token de dispositivo
curl -s https://geo.example.com/v1/tokens \
  -H "Authorization: Bearer $GS_API_KEY" -H "Content-Type: application/json" \
  -d '{"external_subject_id":"u-48213","installation_id":"3f1c2a7e-9b0d-4c1e-8f2a-6d5b4c3a2e10","platform":"ANDROID"}'

# Backend: crear sesión de viaje con conductor y pasajero
curl -s https://geo.example.com/v1/sessions \
  -H "Authorization: Bearer $GS_API_KEY" -H "Content-Type: application/json" \
  -H "Idempotency-Key: trip-9921-session" \
  -d '{"mode":"ONE_TO_ONE","expires_in_seconds":7200,
       "initial_members":[{"external_subject_id":"driver-77"},{"external_subject_id":"rider-12"}]}'

# App (token de dispositivo): empezar a compartir
curl -s -X PUT https://geo.example.com/v1/sessions/$SES/members/me/sharing \
  -H "Authorization: Bearer $DEVICE_TOKEN" -H "Content-Type: application/json" \
  -d '{"sharing":true}'
```
