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

## 10. Webhooks

### 10.1 Modelo
Endpoints registrados por tenant (`/v1/webhooks`). Cada evento de dominio que proyecta a webhook (`05` §10) se escribe en el outbox **en la misma transacción** que el cambio de estado, una fila por endpoint suscrito al tipo. El rol `worker` entrega.

### 10.2 Catálogo de tipos
`session.created`, `session.updated`, `session.closed`, `session.expired`, `member.joined`, `member.updated`, `member.left`, `member.revoked`, `sharing.started`, `sharing.stopped`, `geofence.entered` (F11), `geofence.exited` (F11), `webhook.test`. Esquemas en OpenAPI (`webhooks:`).

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

## 11. Versionado y deprecación
- Cambios aditivos (campos opcionales nuevos en respuestas, endpoints nuevos, valores de enum nuevos documentados como posibles) no cambian `/v1`. Los clientes DEBEN ignorar campos desconocidos en respuestas y tratar valores de enum desconocidos de forma segura.
- Cambios incompatibles → `/v2`, conviviendo con `/v1` durante ≥ 6 meses.
- Endpoints en deprecación responden con `Deprecation: @<unix>` y `Sunset: <fecha HTTP>`.

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
